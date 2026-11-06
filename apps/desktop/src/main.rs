#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod controllers;
mod ui;

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use parabolica_storage::Storage;

use ui::shell::{AppAssets, Shell};

fn main() {
    let storage = match database_path() {
        Some(path) => Storage::open(path).map_err(|error| error.to_string()),
        None => Err("no folder for app data was found".to_owned()),
    };
    let (storage, storage_error) = match storage {
        Ok(storage) => (Some(storage), None),
        Err(error) => (
            None,
            Some(SharedString::from(format!(
                "Library database unavailable: {error}. Changes are not saved."
            ))),
        ),
    };
    let saved_appearance = ui::appearance::load_saved_mode(storage.as_ref());

    application().with_assets(AppAssets).run(move |cx| {
        init(cx);
        ui::shell::init(cx);
        // Resolve the theme before the window exists, so the first frame is drawn in it.
        ui::library::init(storage.clone(), cx);
        ui::appearance::init(saved_appearance, storage, cx);
        ui::accounts::init(cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Parabolica".into()),
                ..TitleBar::title_bar_options()
            }),
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(860.)), cx)),
            window_min_size: Some(size(px(640.), px(480.))),
            app_id: Some("parabolica".into()),
            ..TitleBar::window_options()
        };
        open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(storage_error, window, cx))
        })
        .expect("failed to open the main window");
    });
}

/// `parabolica.db` in the per-user app data folder: `%LOCALAPPDATA%\Parabolica` on Windows,
/// `~/Library/Application Support/Parabolica` on macOS and `$XDG_DATA_HOME/parabolica` on Linux.
fn database_path() -> Option<PathBuf> {
    let env_dir = |name: &str| {
        std::env::var_os(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let folder = if cfg!(windows) {
        env_dir("LOCALAPPDATA")?.join("Parabolica")
    } else if cfg!(target_os = "macos") {
        env_dir("HOME")?.join("Library/Application Support/Parabolica")
    } else {
        env_dir("XDG_DATA_HOME")
            .or_else(|| env_dir("HOME").map(|home| home.join(".local/share")))?
            .join("parabolica")
    };
    Some(folder.join("parabolica.db"))
}
