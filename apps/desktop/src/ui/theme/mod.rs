//! Semantic Light/Dark tokens mapped onto the Kit theme.

use std::rc::Rc;

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeConfig, ThemeConfigColors, ThemeMode};
use parabolica_platform::contrast::HighContrastColors;

/// Color roles from `DESIGN.md`, as `0xRRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub window: u32,
    pub sidebar: u32,
    pub raised: u32,
    pub text: u32,
    pub secondary_text: u32,
    pub rule: u32,
    pub primary: u32,
    pub on_primary: u32,
    pub focus: u32,
    pub link: u32,
    pub selection: u32,
    pub on_selection: u32,
    pub success: u32,
    pub success_surface: u32,
    pub warning: u32,
    pub warning_surface: u32,
}

pub const LIGHT: Palette = Palette {
    window: 0xFAFAF9,
    sidebar: 0xF0F1F2,
    raised: 0xFFFFFF,
    text: 0x24272B,
    secondary_text: 0x62686F,
    rule: 0xDDDFE2,
    primary: 0x24272B,
    on_primary: 0xFFFFFF,
    focus: 0x315897,
    link: 0x315897,
    selection: 0xE8EEF7,
    on_selection: 0x24272B,
    success: 0x34694D,
    success_surface: 0xEDF5EF,
    warning: 0x80591F,
    warning_surface: 0xFBF3E5,
};

pub const DARK: Palette = Palette {
    window: 0x17191C,
    sidebar: 0x202328,
    raised: 0x282C32,
    text: 0xECEEF1,
    secondary_text: 0xADB4BE,
    rule: 0x3B424D,
    primary: 0xECEEF1,
    on_primary: 0x1B2028,
    focus: 0xADCAFF,
    link: 0xADCAFF,
    selection: 0x283B56,
    on_selection: 0xECEEF1,
    success: 0xA7DABA,
    success_surface: 0x253B2D,
    warning: 0xF2CE8C,
    warning_surface: 0x433723,
};

impl Palette {
    /// Windows high-contrast system colors replace every role. Status colors use the plain
    /// text pair, since a contrast theme defines no status colors.
    pub fn high_contrast(colors: &HighContrastColors) -> Self {
        let window = colors.window.to_u32();
        let text = colors.window_text.to_u32();
        Self {
            window,
            sidebar: window,
            raised: window,
            text,
            secondary_text: text,
            rule: text,
            primary: colors.highlight.to_u32(),
            on_primary: colors.highlight_text.to_u32(),
            focus: text,
            link: colors.hyperlink.to_u32(),
            selection: colors.highlight.to_u32(),
            on_selection: colors.highlight_text.to_u32(),
            success: text,
            success_surface: window,
            warning: text,
            warning_surface: window,
        }
    }

    /// Whether the window background is dark, by WCAG relative luminance.
    pub fn is_dark(&self) -> bool {
        relative_luminance(self.window) < 0.5
    }

    fn theme_config(&self, name: &str, mode: ThemeMode) -> ThemeConfig {
        let hex = |color: u32| Some(format!("#{color:06X}").into());
        let mut colors = ThemeConfigColors::default();
        colors.background = hex(self.window);
        colors.foreground = hex(self.text);
        colors.border = hex(self.rule);
        colors.input = hex(self.rule);
        colors.window_border = hex(self.rule);
        colors.muted = hex(self.sidebar);
        colors.muted_foreground = hex(self.secondary_text);
        colors.secondary = hex(self.sidebar);
        colors.secondary_foreground = hex(self.text);
        colors.accent = hex(self.selection);
        colors.accent_foreground = hex(self.on_selection);
        colors.primary = hex(self.primary);
        colors.primary_foreground = hex(self.on_primary);
        colors.ring = hex(self.focus);
        colors.caret = hex(self.text);
        colors.link = hex(self.link);
        colors.selection = hex(self.selection);
        colors.popover = hex(self.raised);
        colors.popover_foreground = hex(self.text);
        colors.list = hex(self.window);
        colors.list_active = hex(self.selection);
        colors.list_active_border = hex(self.focus);
        colors.sidebar = hex(self.sidebar);
        colors.sidebar_foreground = hex(self.text);
        colors.sidebar_border = hex(self.rule);
        colors.sidebar_accent = hex(self.selection);
        colors.sidebar_accent_foreground = hex(self.on_selection);
        colors.title_bar = hex(self.sidebar);
        colors.title_bar_border = hex(self.rule);
        colors.status_bar = hex(self.sidebar);
        colors.status_bar_border = hex(self.rule);
        colors.success = hex(self.success);
        colors.success_foreground = hex(self.success_surface);
        colors.warning = hex(self.warning);
        colors.warning_foreground = hex(self.warning_surface);

        let mut config = ThemeConfig::default();
        config.name = name.to_owned().into();
        config.mode = mode;
        config.font_size = Some(15.);
        config.radius = Some(6);
        config.radius_lg = Some(12);
        config.colors = colors;
        config
    }
}

/// Installs Parabolica's palettes and shows `mode`. An active high-contrast theme takes
/// precedence over `mode`, and its background decides light or dark.
pub fn apply(mode: ThemeMode, high_contrast: Option<&HighContrastColors>, cx: &mut App) {
    let (light, dark, mode) = match high_contrast {
        Some(colors) => {
            let palette = Palette::high_contrast(colors);
            let mode = if palette.is_dark() {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            (palette, palette, mode)
        }
        None => (LIGHT, DARK, mode),
    };
    let light = Rc::new(light.theme_config("Parabolica Light", ThemeMode::Light));
    let dark = Rc::new(dark.theme_config("Parabolica Dark", ThemeMode::Dark));
    Theme::global_mut(cx).light_theme = light;
    Theme::global_mut(cx).dark_theme = dark;
    Theme::change(mode, None, cx);
}

fn relative_luminance(color: u32) -> f64 {
    let channel = |shift: u32| {
        let value = f64::from((color >> shift) & 0xFF) / 255.;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

#[cfg(test)]
mod tests {
    use parabolica_platform::contrast::Rgb;

    use super::*;

    fn contrast(a: u32, b: u32) -> f64 {
        let (a, b) = (relative_luminance(a), relative_luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn text_pairs_meet_normal_text_contrast() {
        for palette in [LIGHT, DARK] {
            let pairs = [
                (palette.text, palette.window),
                (palette.text, palette.sidebar),
                (palette.text, palette.raised),
                (palette.secondary_text, palette.window),
                (palette.secondary_text, palette.sidebar),
                (palette.on_primary, palette.primary),
                (palette.on_selection, palette.selection),
                (palette.link, palette.window),
                (palette.success, palette.success_surface),
                (palette.warning, palette.warning_surface),
            ];
            for (foreground, background) in pairs {
                let ratio = contrast(foreground, background);
                assert!(
                    ratio >= 4.5,
                    "#{foreground:06X} on #{background:06X} is {ratio:.2}:1"
                );
            }
        }
    }

    #[test]
    fn high_contrast_dark_theme_resolves_dark() {
        let black = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        let colors = HighContrastColors {
            window: black,
            window_text: white,
            highlight: Rgb::new(0x1A, 0xEB, 0xFF),
            highlight_text: black,
            hyperlink: Rgb::new(0xFF, 0xFF, 0x00),
        };
        let palette = Palette::high_contrast(&colors);
        assert!(palette.is_dark());
        assert_eq!(palette.text, 0xFFFFFF);
        assert_eq!(palette.selection, 0x1AEBFF);
        assert!(!LIGHT.is_dark());
        assert!(DARK.is_dark());
    }
}
