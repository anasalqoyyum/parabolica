//! Application icons beyond the component set Kit embeds by default.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Result, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Disc3,
        ListMusic,
        Music,
        Repeat,
        ScanLine,
        Shuffle,
        SkipBack,
        SkipForward,
        SlidersHorizontal,
        SunMoon,
        Volume2,
        X,
    ]
);

#[derive(Debug, Clone, Copy)]
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::assets::IconName;

    #[test]
    fn queue_navigation_and_remove_icons_are_bundled() {
        for icon in [IconName::ListMusic, IconName::X] {
            let path = icon.path();
            let bytes = AppAssets.load(&path).unwrap();
            assert!(bytes.is_some(), "missing application icon: {path}");
        }
    }
}
