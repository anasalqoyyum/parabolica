use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowAppearance,
    WindowBounds, WindowOptions, px, size,
};
use parabolica_storage::Storage;

use super::{Page, SEARCH_KEY, SettingsSection, Shell};
use crate::ui::appearance::{self, AppearanceMode};

fn open_shell(
    cx: &mut TestAppContext,
    width: f32,
    saved: Option<AppearanceMode>,
    storage: Option<Storage>,
) -> (AnyWindowHandle, Entity<Shell>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
        crate::ui::library::init(storage.clone(), cx);
        appearance::init(saved, storage, cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(width), px(800.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Shell::new(None, window, cx))
        })
        .expect("open the shell window")
    })
}

fn theme_mode(cx: &mut TestAppContext) -> ThemeMode {
    cx.update(|cx| Theme::global(cx).mode)
}

#[gpui_kit::test]
fn tab_reaches_navigation_and_enter_opens_the_focused_page(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 1280., None, None);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Back and Forward are disabled on the first page and are skipped.
        for id in [
            "nav-albums",
            "nav-songs",
            "nav-artists",
            "nav-queue",
            "nav-playlists",
            "nav-replaygain",
            "nav-sources",
            "search",
            "open-settings",
            "open-appearance",
        ] {
            window.press("tab", cx);
            assert_eq!(
                window.find(id).focused(),
                Some(true),
                "Tab should reach {id}"
            );
        }

        window.press("shift-tab", cx);
        window.press("shift-tab", cx);
        window.press("shift-tab", cx);
        assert_eq!(window.find("nav-sources").focused(), Some(true));
        window.press("shift-tab", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).page(), Page::ReplayGain));
}

#[gpui_kit::test]
fn settings_sections_and_theme_choices_are_keyboard_operable(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 1280., Some(AppearanceMode::Light), None);
    cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Settings(SettingsSection::Appearance), cx)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let sections = [
            "settings-sources",
            "settings-listening-device",
            "settings-playback",
            "settings-replaygain",
            "settings-library",
            "settings-appearance",
            "settings-shortcuts",
            "settings-about",
        ];
        for _ in 0..11 {
            window.press("tab", cx);
        }
        assert_eq!(window.find("open-appearance").focused(), Some(true));
        for id in sections {
            window.press("tab", cx);
            assert_eq!(
                window.find(id).focused(),
                Some(true),
                "Tab should reach {id}"
            );
        }
        assert_eq!(window.find("settings-appearance").selected(), Some(true));

        // Follow system, Light, Dark; Space selects the focused choice.
        window.press("tab", cx);
        window.press("tab", cx);
        window.press("tab", cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(appearance::mode(cx), AppearanceMode::Dark));
    assert_eq!(theme_mode(cx), ThemeMode::Dark);
}

#[gpui_kit::test]
fn search_shortcut_focuses_search(cx: &mut TestAppContext) {
    let (window, _shell) = open_shell(cx, 1280., None, None);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(SEARCH_KEY, cx);
        assert_eq!(window.find("search").focused(), Some(true));
        window.input("夜に駆ける", cx);
        assert_eq!(window.find("search").value(), Some("夜に駆ける"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn back_and_forward_follow_visited_pages(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 1280., None, None);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-songs", cx);
        window.click("nav-artists", cx);
        window.press("alt-left", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).page(), Page::Songs));
    cx.update_window(window, |_, window, cx| {
        window.click("back", cx);
        window.click("forward", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).page(), Page::Songs));
}

#[gpui_kit::test]
fn narrow_windows_collapse_the_sidebar_until_revealed(cx: &mut TestAppContext) {
    let (window, _shell) = open_shell(cx, 900., None, None);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let rail = window.find("sidebar").bounds().size.width;
        assert!(rail < px(100.), "rail is {rail:?}");
        assert_eq!(window.find("nav-albums").label(), Some("Albums"));

        window.click("sidebar-toggle", cx);
        assert!(window.find("sidebar").bounds().size.width > px(200.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn explicit_modes_ignore_system_changes_and_follow_system_tracks_them(cx: &mut TestAppContext) {
    let (_window, _shell) = open_shell(cx, 1280., Some(AppearanceMode::Light), None);
    cx.update(|cx| appearance::system_appearance_changed(WindowAppearance::Dark, cx));
    assert_eq!(theme_mode(cx), ThemeMode::Light);

    cx.update(|cx| appearance::select(AppearanceMode::FollowSystem, cx));
    assert_eq!(theme_mode(cx), ThemeMode::Dark);
    cx.update(|cx| appearance::system_appearance_changed(WindowAppearance::Light, cx));
    assert_eq!(theme_mode(cx), ThemeMode::Light);

    cx.update(|cx| appearance::select(AppearanceMode::Dark, cx));
    cx.update(|cx| appearance::system_appearance_changed(WindowAppearance::Light, cx));
    assert_eq!(theme_mode(cx), ThemeMode::Dark);
}

#[gpui_kit::test]
fn theme_changes_keep_the_page_and_focus(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 1280., Some(AppearanceMode::FollowSystem), None);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("nav-songs", cx);
        window.press(SEARCH_KEY, cx);
        window.input("Shabadaba", cx);
    })
    .unwrap();
    cx.update(|cx| appearance::system_appearance_changed(WindowAppearance::Dark, cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("search").focused(), Some(true));
        assert_eq!(window.find("search").value(), Some("Shabadaba"));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).page(), Page::Songs));
    assert_eq!(theme_mode(cx), ThemeMode::Dark);
}

#[gpui_kit::test]
fn the_last_selection_is_saved(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::open(dir.path().join("parabolica.db")).unwrap();
    let (_window, _shell) = open_shell(cx, 1280., None, Some(storage.clone()));

    cx.update(|cx| {
        appearance::select(AppearanceMode::Dark, cx);
        appearance::select(AppearanceMode::Light, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        storage.preference::<AppearanceMode>().unwrap(),
        Some(AppearanceMode::Light)
    );
    cx.update(|cx| assert_eq!(appearance::save_error(cx), None));
}

#[gpui_kit::test]
fn shortcuts_work_after_the_focused_element_leaves_the_page(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 1280., None, None);
    cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Settings(SettingsSection::Appearance), cx)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for _ in 0..12 {
            window.press("tab", cx);
        }
        assert_eq!(window.find("settings-sources").focused(), Some(true));
        // The focused settings item is not rendered on the previous page.
        window.press("alt-left", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(shell.read(cx).page(), Page::Albums));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("alt-right", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            shell.read(cx).page(),
            Page::Settings(SettingsSection::Appearance)
        )
    });
}

#[gpui_kit::test]
fn theme_choices_fit_the_minimum_window_width(cx: &mut TestAppContext) {
    let (window, shell) = open_shell(cx, 640., None, None);
    cx.update(|cx| {
        shell.update(cx, |shell, cx| {
            shell.navigate(Page::Settings(SettingsSection::Appearance), cx)
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let content = window.find("content").bounds();
        let group = window.within("appearance-mode");
        for index in 0..3usize {
            let choice = group.find(index).bounds();
            assert!(
                choice.right() <= content.right(),
                "choice {index} is clipped"
            );
        }
    })
    .unwrap();
}
