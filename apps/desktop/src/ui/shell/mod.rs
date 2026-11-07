//! Main window layout: title bar, sidebar, top bar, page slot and player bar.

mod icons;
mod nav;
mod page;
#[cfg(test)]
mod tests;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Selectable as _, StyledExt as _, TitleBar, h_flex,
    v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, KeyBinding, Pixels, Render, Role, SharedString,
    Subscription, TestSupportExt as _, Window, actions, div, px,
};

pub use icons::AppAssets;
pub(crate) use nav::NavItem;
use page::{History, NAV_GROUPS};
pub use page::{Page, SettingsSection};

use crate::ui::accounts::AccountsSettings;
use crate::ui::appearance::{self, AppearanceSettings};
use crate::ui::library::LibraryView;
use crate::ui::player::Player;
use parabolica_storage::collection::BrowseKind;

actions!(shell, [FocusSearch, GoBack, GoForward]);

const CONTEXT: &str = "Shell";
/// At or below this width the sidebar collapses to an icon rail.
const NARROW_WIDTH: Pixels = px(1024.);
const SIDEBAR_WIDTH: Pixels = px(224.);
const RAIL_WIDTH: Pixels = px(64.);
const SEARCH_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-f"
} else {
    "ctrl-f"
};

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(SEARCH_KEY, FocusSearch, Some(CONTEXT)),
        KeyBinding::new("alt-left", GoBack, Some(CONTEXT)),
        KeyBinding::new("alt-right", GoForward, Some(CONTEXT)),
    ]);
}

pub struct Shell {
    focus_handle: FocusHandle,
    history: History,
    search: Entity<InputState>,
    /// Whether the user expanded the sidebar while the window is narrow.
    sidebar_revealed: bool,
    storage_error: Option<SharedString>,
    player: Entity<Player>,
    library: Entity<LibraryView>,
    queue: Entity<crate::ui::queue::QueueView>,
    playlists: Entity<crate::ui::playlists::PlaylistsView>,
    _subscriptions: [Subscription; 4],
}

impl Shell {
    pub fn new(
        storage_error: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search songs, albums, artists"));
        let player = cx.new(Player::new);
        let commands = player.read(cx).queue();
        let queue = cx.new(|cx| crate::ui::queue::QueueView::new(commands.clone(), cx));
        let playlists = cx.new(|cx| crate::ui::playlists::PlaylistsView::new(commands, window, cx));
        let library = cx.new(|cx| LibraryView::new(player.clone(), cx));
        let search_library = library.clone();
        let subscriptions = [
            cx.subscribe(
                &search,
                move |_, input, _: &gpui_kit::component::input::InputEvent, cx| {
                    let text = input.read(cx).value().to_string();
                    search_library.update(cx, |library, cx| library.search(text, cx));
                },
            ),
            cx.observe_window_appearance(window, |_, window, cx| {
                appearance::system_appearance_changed(window.appearance(), cx);
            }),
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    appearance::refresh_system_settings(cx);
                }
            }),
            // When the focused element stops rendering, GPUI dispatches keys from the window
            // root, outside the shell's key context. Keep focus inside the shell.
            cx.on_focus_lost(window, |this, window, cx| {
                this.focus_handle.focus(window, cx)
            }),
        ];
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        Self {
            focus_handle,
            history: History::new(Page::Albums),
            search,
            sidebar_revealed: false,
            storage_error,
            player,
            library,
            queue,
            playlists,
            _subscriptions: subscriptions,
        }
    }

    pub fn page(&self) -> Page {
        self.history.current()
    }

    pub fn navigate(&mut self, page: Page, cx: &mut Context<Self>) {
        self.history.visit(page);
        self.sync_library_page(cx);
        cx.notify();
    }

    fn sync_library_page(&mut self, cx: &mut Context<Self>) {
        let page = self.page();
        let target = match page {
            Page::Albums => Some((BrowseKind::Albums, false)),
            Page::Songs => Some((BrowseKind::Songs, false)),
            Page::Artists => Some((BrowseKind::Artists, false)),
            Page::Settings(SettingsSection::SourcesAndOutput | SettingsSection::Library) => {
                Some((BrowseKind::Folders, true))
            }
            _ => None,
        };
        if let Some((kind, settings)) = target {
            self.library
                .update(cx, |library, cx| library.show(kind, settings, cx));
        }
    }

    fn focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.focus(window, cx));
    }

    fn go_back(&mut self, _: &GoBack, _: &mut Window, cx: &mut Context<Self>) {
        self.history.back();
        self.sync_library_page(cx);
        cx.notify();
    }

    fn go_forward(&mut self, _: &GoForward, _: &mut Window, cx: &mut Context<Self>) {
        self.history.forward();
        self.sync_library_page(cx);
        cx.notify();
    }

    fn render_sidebar(&self, narrow: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = narrow && !self.sidebar_revealed;
        let page = self.page();
        let theme = cx.theme();

        v_flex()
            .id("sidebar")
            .test_support()
            .role(Role::Navigation)
            .aria_label("Library")
            .flex_shrink_0()
            .h_full()
            .w(if collapsed { RAIL_WIDTH } else { SIDEBAR_WIDTH })
            .px_3()
            .py_4()
            .gap_5()
            .overflow_y_scroll()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.sidebar_border)
            .when(narrow, |this| {
                let (icon, label) = if collapsed {
                    (IconName::PanelLeftOpen, "Show sidebar")
                } else {
                    (IconName::PanelLeftClose, "Hide sidebar")
                };
                this.child(
                    Button::new("sidebar-toggle")
                        .ghost()
                        .icon(icon)
                        .accessibility_label(label)
                        .tooltip(label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sidebar_revealed = !this.sidebar_revealed;
                            cx.notify();
                        })),
                )
            })
            .children(NAV_GROUPS.iter().map(|group| {
                v_flex()
                    .gap_1()
                    .when(!collapsed, |this| {
                        this.child(
                            div()
                                .px_3()
                                .pb_1()
                                .text_xs()
                                .font_semibold()
                                .text_color(theme.muted_foreground)
                                .child(group.label.to_uppercase()),
                        )
                    })
                    .child(
                        v_flex()
                            .id(group.label)
                            .role(Role::List)
                            .aria_label(group.label)
                            .gap_1()
                            .children(group.entries.iter().map(|entry| {
                                let target = entry.page;
                                NavItem::new(
                                    entry.id,
                                    entry.label,
                                    entry.is_current(page),
                                    cx.listener(move |this, _, _, cx| this.navigate(target, cx)),
                                )
                                .icon(entry.icon)
                                .collapsed(collapsed)
                            })),
                    )
            }))
    }

    fn render_top_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shortcut = Kbd::binding_for_action(&FocusSearch, Some(CONTEXT), window);
        let page = self.page();

        h_flex()
            .id("top-bar")
            .flex_shrink_0()
            .h(px(68.))
            .px_8()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("back")
                    .ghost()
                    .icon(IconName::ChevronLeft)
                    .accessibility_label("Back")
                    .tooltip("Back")
                    .disabled(!self.history.can_go_back())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_back(&GoBack, window, cx)
                    })),
            )
            .child(
                Button::new("forward")
                    .ghost()
                    .icon(IconName::ChevronRight)
                    .accessibility_label("Forward")
                    .tooltip("Forward")
                    .disabled(!self.history.can_go_forward())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.go_forward(&GoForward, window, cx)
                    })),
            )
            .child(
                Input::new(&self.search)
                    .id("search")
                    .aria_label("Search")
                    .ml_4()
                    .w(px(430.))
                    .min_w(px(160.))
                    .flex_shrink(1.)
                    .prefix(Icon::new(IconName::Search).text_color(cx.theme().muted_foreground))
                    .when_some(shortcut, |input, shortcut| input.suffix(shortcut)),
            )
            .child(div().flex_1())
            .child(
                Button::new("open-settings")
                    .ghost()
                    .icon(IconName::Settings2)
                    .accessibility_label("Settings")
                    .tooltip("Settings")
                    .selected(matches!(page, Page::Settings(section) if section != SettingsSection::Appearance))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Settings(SettingsSection::SourcesAndOutput), cx)
                    })),
            )
            .child(
                Button::new("open-appearance")
                    .ghost()
                    .icon(IconName::SunMoon)
                    .accessibility_label("Appearance")
                    .tooltip("Appearance")
                    .selected(page == Page::Settings(SettingsSection::Appearance))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.navigate(Page::Settings(SettingsSection::Appearance), cx)
                    })),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let page = self.page();
        let theme = cx.theme();

        v_flex()
            .id("content")
            .test_support()
            .role(Role::Main)
            .flex_1()
            .min_h_0()
            .when(
                matches!(page, Page::Settings(_) | Page::ReplayGain),
                |view| view.overflow_y_scroll(),
            )
            .px_8()
            .py_8()
            .gap_6()
            .when_some(self.storage_error.clone(), |this, error| {
                this.child(
                    h_flex()
                        .id("storage-error")
                        .role(Role::Alert)
                        .aria_label(error.clone())
                        .gap_3()
                        .p_3()
                        .rounded(px(8.))
                        .bg(theme.warning_foreground)
                        .text_color(theme.warning)
                        .child(Icon::new(IconName::TriangleAlert))
                        .child(error),
                )
            })
            .child(
                div()
                    .id("page-title")
                    .role(Role::Heading)
                    .aria_level(1)
                    .aria_label(page.title())
                    .text_size(px(30.))
                    .font_semibold()
                    .child(page.title()),
            )
            .child(match page {
                Page::Settings(section) => self.render_settings(section, cx),
                Page::Albums | Page::Songs | Page::Artists => {
                    self.library.clone().into_any_element()
                }
                Page::Queue => self.queue.clone().into_any_element(),
                Page::Playlists => self.playlists.clone().into_any_element(),
                Page::ReplayGain => not_available(cx),
            })
    }

    fn render_settings(&self, current: SettingsSection, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .items_start()
            .gap_8()
            .child(
                v_flex()
                    .id("settings-sections")
                    .role(Role::List)
                    .aria_label("Settings")
                    .flex_shrink_0()
                    .w(px(170.))
                    .gap_1()
                    .children(SettingsSection::ALL.map(|section| {
                        NavItem::new(
                            settings_id(section),
                            section.title(),
                            section == current,
                            cx.listener(move |this, _, _, cx| {
                                this.navigate(Page::Settings(section), cx)
                            }),
                        )
                    })),
            )
            .child(
                div().flex_1().min_w_0().child(match current {
                    SettingsSection::SourcesAndOutput => v_flex()
                        .gap_6()
                        .child(self.library.clone())
                        .child(AccountsSettings)
                        .into_any_element(),
                    SettingsSection::Library => self.library.clone().into_any_element(),
                    SettingsSection::Appearance => AppearanceSettings.into_any_element(),
                    _ => not_available(cx),
                }),
            )
            .into_any_element()
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width <= NARROW_WIDTH;
        let theme = cx.theme();

        v_flex()
            .id("shell")
            .key_context(CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::focus_search))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::go_forward))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(TitleBar::new().child(div().text_sm().font_semibold().child("Parabolica")))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.render_sidebar(narrow, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .child(self.render_top_bar(window, cx))
                            .child(self.render_content(cx)),
                    ),
            )
            .child(self.player.clone())
    }
}

fn settings_id(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::SourcesAndOutput => "settings-sources",
        SettingsSection::ListeningDevice => "settings-listening-device",
        SettingsSection::Playback => "settings-playback",
        SettingsSection::ReplayGain => "settings-replaygain",
        SettingsSection::Library => "settings-library",
        SettingsSection::Appearance => "settings-appearance",
        SettingsSection::Shortcuts => "settings-shortcuts",
        SettingsSection::About => "settings-about",
    }
}

fn not_available(cx: &App) -> AnyElement {
    const TEXT: &str = "Not available yet";
    div()
        .id("not-available")
        .role(Role::Label)
        .aria_value(TEXT)
        .text_color(cx.theme().muted_foreground)
        .child(TEXT)
        .into_any_element()
}
