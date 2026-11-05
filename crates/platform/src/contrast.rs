//! The operating system's high-contrast setting.

/// A 24-bit sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `0xRRGGBB`.
    pub const fn to_u32(self) -> u32 {
        (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
}

/// System colors of the active high-contrast theme. Windows defines these roles; text drawn
/// in one role's foreground on its background meets the theme's contrast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighContrastColors {
    pub window: Rgb,
    pub window_text: Rgb,
    pub highlight: Rgb,
    pub highlight_text: Rgb,
    pub hyperlink: Rgb,
}

/// The active high-contrast colors, or `None` when high contrast is off or the platform does not
/// report it.
pub fn high_contrast_colors() -> Option<HighContrastColors> {
    imp::high_contrast_colors()
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod imp {
    use windows::Win32::Graphics::Gdi::{
        COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT, COLOR_WINDOW, COLOR_WINDOWTEXT,
        GetSysColor, SYS_COLOR_INDEX,
    };
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };

    use super::{HighContrastColors, Rgb};

    pub(super) fn high_contrast_colors() -> Option<HighContrastColors> {
        let mut setting = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        // SAFETY: SPI_GETHIGHCONTRAST writes one HIGHCONTRASTW, whose size is in `cbSize`.
        unsafe {
            SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                setting.cbSize,
                Some((&raw mut setting).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .ok()?;
        if !setting.dwFlags.contains(HCF_HIGHCONTRASTON) {
            return None;
        }
        Some(HighContrastColors {
            window: system_color(COLOR_WINDOW),
            window_text: system_color(COLOR_WINDOWTEXT),
            highlight: system_color(COLOR_HIGHLIGHT),
            highlight_text: system_color(COLOR_HIGHLIGHTTEXT),
            hyperlink: system_color(COLOR_HOTLIGHT),
        })
    }

    fn system_color(index: SYS_COLOR_INDEX) -> Rgb {
        // SAFETY: GetSysColor reads a process-wide table and accepts every COLOR_* index.
        let colorref = unsafe { GetSysColor(index) };
        // COLORREF is 0x00BBGGRR.
        Rgb::new(
            colorref as u8,
            (colorref >> 8) as u8,
            (colorref >> 16) as u8,
        )
    }
}

#[cfg(not(windows))]
mod imp {
    pub(super) fn high_contrast_colors() -> Option<super::HighContrastColors> {
        None
    }
}
