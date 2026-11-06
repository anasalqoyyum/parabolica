//! Follow system, Light and Dark appearance settings.

mod page;

use gpui_kit::component::ThemeMode;
use gpui_kit::{App, Global, SharedString, WindowAppearance};
use parabolica_platform::contrast::{self, HighContrastColors};
use parabolica_storage::Storage;
use parabolica_storage::preferences::Preference;

pub use page::AppearanceSettings;

use crate::ui::theme;

/// The user's saved choice, kept separate from the light/dark theme it resolves to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AppearanceMode {
    #[default]
    FollowSystem,
    Light,
    Dark,
}

impl AppearanceMode {
    pub const ALL: [Self; 3] = [Self::FollowSystem, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::FollowSystem => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    pub fn resolve(self, system: WindowAppearance) -> ThemeMode {
        match self {
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
            Self::FollowSystem => match system {
                WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
                WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            },
        }
    }
}

impl Preference for AppearanceMode {
    const KEY: &'static str = "appearance.mode";

    fn encode(&self) -> String {
        match self {
            Self::FollowSystem => "follow_system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
        .to_owned()
    }

    fn decode(stored: &str) -> Option<Self> {
        match stored {
            "follow_system" => Some(Self::FollowSystem),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }
}

/// App-wide appearance state. The theme is derived from it whenever an input changes.
#[derive(Debug)]
struct Appearance {
    mode: AppearanceMode,
    system: WindowAppearance,
    high_contrast: Option<HighContrastColors>,
    storage: Option<Storage>,
    /// The mode most recently written to storage, or read from it at startup.
    persisted: Option<AppearanceMode>,
    saving: bool,
    save_error: Option<SharedString>,
}

impl Global for Appearance {}

/// Read the saved mode before the first window opens, so it draws in the right theme.
///
/// Blocks on storage once at startup, before any window exists. An unreadable stored value is
/// treated as unset.
pub fn load_saved_mode(storage: Option<&Storage>) -> Option<AppearanceMode> {
    storage?.preference::<AppearanceMode>().ok().flatten()
}

pub fn init(saved: Option<AppearanceMode>, storage: Option<Storage>, cx: &mut App) {
    cx.set_global(Appearance {
        mode: saved.unwrap_or_default(),
        system: cx.window_appearance(),
        high_contrast: contrast::high_contrast_colors(),
        storage,
        persisted: saved,
        saving: false,
        save_error: None,
    });
    apply_theme(cx);
}

pub fn mode(cx: &App) -> AppearanceMode {
    cx.global::<Appearance>().mode
}

/// Whether a Windows high-contrast theme is overriding the selected mode.
pub fn high_contrast_active(cx: &App) -> bool {
    cx.global::<Appearance>().high_contrast.is_some()
}

pub fn save_error(cx: &App) -> Option<SharedString> {
    cx.global::<Appearance>().save_error.clone()
}

pub fn select(mode: AppearanceMode, cx: &mut App) {
    cx.global_mut::<Appearance>().mode = mode;
    apply_theme(cx);
    persist(cx);
}

/// The OS app theme changed. Only Follow system reacts.
pub fn system_appearance_changed(system: WindowAppearance, cx: &mut App) {
    let state = cx.global_mut::<Appearance>();
    if state.system == system {
        return;
    }
    state.system = system;
    if state.mode == AppearanceMode::FollowSystem && state.high_contrast.is_none() {
        apply_theme(cx);
    }
}

/// Re-read OS accessibility settings that Windows does not announce to GPUI: high contrast and
/// animation effects. Called when a window becomes active.
pub fn refresh_system_settings(cx: &mut App) {
    gpui_kit::base::apply_system_reduce_motion(cx);
    let high_contrast = contrast::high_contrast_colors();
    let state = cx.global_mut::<Appearance>();
    if state.high_contrast != high_contrast {
        state.high_contrast = high_contrast;
        apply_theme(cx);
    }
}

fn apply_theme(cx: &mut App) {
    let state = cx.global::<Appearance>();
    let mode = state.mode.resolve(state.system);
    let high_contrast = state.high_contrast;
    theme::apply(mode, high_contrast.as_ref(), cx);
}

/// Saves the current mode off the UI thread. One save runs at a time; a selection made while a
/// save is in flight is written when it finishes, so the last choice is the one stored.
fn persist(cx: &mut App) {
    let state = cx.global_mut::<Appearance>();
    if state.saving || state.persisted == Some(state.mode) {
        return;
    }
    let Some(storage) = state.storage.clone() else {
        return;
    };
    let mode = state.mode;
    state.saving = true;

    let save = cx
        .background_executor()
        .spawn(async move { storage.save_preference(&mode) });
    cx.spawn(async move |cx| {
        let result = save.await;
        cx.update(|cx| {
            let state = cx.global_mut::<Appearance>();
            state.saving = false;
            match result {
                Ok(()) => {
                    state.persisted = Some(mode);
                    state.save_error = None;
                }
                Err(error) => {
                    state.save_error = Some(format!("Appearance not saved: {error}").into());
                }
            }
            let newer_selection = state.mode != mode;
            cx.refresh_windows();
            if newer_selection {
                persist(cx);
            }
        });
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_text_round_trips() {
        for mode in AppearanceMode::ALL {
            assert_eq!(AppearanceMode::decode(&mode.encode()), Some(mode));
        }
        assert_eq!(AppearanceMode::decode("system"), None);
    }

    #[test]
    fn explicit_modes_ignore_the_system_appearance() {
        for system in [WindowAppearance::Light, WindowAppearance::Dark] {
            assert_eq!(AppearanceMode::Light.resolve(system), ThemeMode::Light);
            assert_eq!(AppearanceMode::Dark.resolve(system), ThemeMode::Dark);
        }
        assert_eq!(
            AppearanceMode::FollowSystem.resolve(WindowAppearance::Dark),
            ThemeMode::Dark
        );
        assert_eq!(
            AppearanceMode::FollowSystem.resolve(WindowAppearance::VibrantLight),
            ThemeMode::Light
        );
    }
}
