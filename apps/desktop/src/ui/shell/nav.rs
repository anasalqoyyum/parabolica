//! Navigation list items for the sidebar and settings sections.
//!
//! Kit's `SidebarMenuItem` cannot take keyboard focus, so these are focusable list items:
//! Tab reaches each one, Enter or Space opens it, and the current page reports as selected.

use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, StyledExt as _, ThemeStyled as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, FocusHandle, Role, SharedString, TestSupportExt as _, Window, div, px,
};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct NavItem {
    id: SharedString,
    label: SharedString,
    icon: Option<IconName>,
    current: bool,
    collapsed: bool,
    on_click: ClickHandler,
}

impl NavItem {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        current: bool,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            current,
            collapsed: false,
            on_click: Box::new(on_click),
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Show only the icon. The label becomes a tooltip and stays the accessible name.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus_handle: FocusHandle = window
            .use_keyed_state(self.id.clone(), cx, |_, cx| {
                cx.focus_handle().tab_stop(true)
            })
            .read(cx)
            .clone();
        let focused = focus_handle.is_focused(window);
        let theme = cx.theme();
        let (hover, selected, on_selected) = (
            theme.sidebar_accent.opacity(0.6),
            theme.sidebar_accent,
            theme.sidebar_accent_foreground,
        );

        h_flex()
            .id(self.id)
            .test_support()
            .role(Role::ListItem)
            .aria_label(self.label.clone())
            .aria_selected(self.current)
            .track_focus(&focus_handle)
            .h(px(40.))
            .px_3()
            .gap_3()
            .rounded(px(8.))
            .text_color(theme.sidebar_foreground)
            .when(self.collapsed, |this| this.justify_center().px_0())
            .when(!self.current, |this| {
                this.hover(move |style| style.bg(hover))
            })
            .when(self.current, |this| {
                this.bg(selected).text_color(on_selected).font_medium()
            })
            .when(focused, |this| this.focus_ring_style(window, cx))
            .when_some(self.icon, |this, icon| this.child(Icon::new(icon).size_4()))
            .when(self.collapsed, |this| {
                let label = self.label.clone();
                this.tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            })
            .when(!self.collapsed, |this| {
                this.child(
                    div()
                        .flex_1()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .child(self.label),
                )
            })
            .on_click(self.on_click)
    }
}
