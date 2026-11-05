#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod controllers;
mod ui;

use gpui_kit::*;

fn main() {
    application().with_assets(assets::Assets).run(|cx| {
        init(cx);

        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Parabolica".into()),
                ..Default::default()
            }),
            app_id: Some("parabolica".into()),
            ..Default::default()
        };
        open_window(options, cx, |_, cx| cx.new(|_| ui::shell::Shell))
            .expect("failed to open the main window");
    });
}
