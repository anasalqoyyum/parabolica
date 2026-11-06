use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Hsla, Pixels, Window, div, px, rgb};

use super::AppearanceMode;
use crate::ui::theme::{DARK, LIGHT, Palette};

const PREVIEW_WIDTH: Pixels = px(200.);
const PREVIEW_HEIGHT: Pixels = px(100.);

/// Settings > Appearance.
#[derive(IntoElement)]
pub struct AppearanceSettings;

impl RenderOnce for AppearanceSettings {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let selected = super::mode(cx);
        let selected_index = AppearanceMode::ALL
            .iter()
            .position(|mode| *mode == selected);
        let theme = cx.theme();
        let (ring, border, raised) = (theme.ring, theme.border, theme.popover);

        v_flex()
            .gap_4()
            .child(
                div()
                    .id("color-theme-heading")
                    .role(gpui_kit::Role::Heading)
                    .aria_level(2)
                    .aria_label("Color theme")
                    .text_size(px(22.))
                    .font_semibold()
                    .child("Color theme"),
            )
            .child(
                RadioGroup::horizontal("appearance-mode")
                    .selected_index(selected_index)
                    .on_click(|index, _, cx| super::select(AppearanceMode::ALL[*index], cx))
                    .children(AppearanceMode::ALL.map(|mode| {
                        let checked = mode == selected;
                        Radio::new(mode.label())
                            .label(mode.label())
                            .p_3()
                            .rounded(px(12.))
                            .border_1()
                            .border_color(border)
                            .bg(raised)
                            .when(checked, |radio| radio.border_2().border_color(ring))
                            .child(preview(mode))
                    })),
            )
            .when(super::high_contrast_active(cx), |this| {
                const TEXT: &str = "High contrast is on. Windows contrast colors are used.";
                this.child(
                    div()
                        .id("high-contrast-active")
                        .role(gpui_kit::Role::Status)
                        .aria_label(TEXT)
                        .text_color(cx.theme().muted_foreground)
                        .child(TEXT),
                )
            })
            .when_some(super::save_error(cx), |this, error| {
                this.child(
                    div()
                        .id("appearance-save-error")
                        .role(gpui_kit::Role::Status)
                        .aria_label(error.clone())
                        .text_color(cx.theme().warning)
                        .child(error),
                )
            })
    }
}

/// A miniature window in the theme it represents, independent of the current theme.
fn preview(mode: AppearanceMode) -> AnyElement {
    let frame = div()
        .mt_2()
        .w(PREVIEW_WIDTH)
        .h(PREVIEW_HEIGHT)
        .rounded(px(6.))
        .overflow_hidden()
        .border_1();
    match mode {
        AppearanceMode::FollowSystem => frame
            .border_color(color(LIGHT.rule))
            .child(
                h_flex()
                    .size_full()
                    .child(miniature(&LIGHT, 1))
                    .child(miniature(&DARK, 1)),
            )
            .into_any_element(),
        AppearanceMode::Light => frame
            .border_color(color(LIGHT.rule))
            .child(miniature(&LIGHT, 3))
            .into_any_element(),
        AppearanceMode::Dark => frame
            .border_color(color(DARK.rule))
            .child(miniature(&DARK, 3))
            .into_any_element(),
    }
}

fn miniature(palette: &Palette, tiles: usize) -> impl IntoElement {
    let line = |width: f32| {
        div()
            .w(px(width))
            .h(px(3.))
            .bg(color(palette.secondary_text))
    };
    let tile_colors = [0xB7C4D9, 0xA3B5A8, 0xC5A3AA];
    h_flex()
        .flex_1()
        .h_full()
        .bg(color(palette.window))
        .child(
            v_flex()
                .h_full()
                .w(px(36.))
                .p_2()
                .gap_1p5()
                .bg(color(palette.sidebar))
                .children((0..3).map(|_| line(16.))),
        )
        .child(
            v_flex()
                .flex_1()
                .h_full()
                .p_2()
                .gap_2()
                .child(line(48.))
                .child(
                    h_flex().gap_1p5().children(
                        tile_colors
                            .iter()
                            .take(tiles)
                            .map(|tile| div().size(px(28.)).rounded(px(2.)).bg(color(*tile))),
                    ),
                ),
        )
}

fn color(value: u32) -> Hsla {
    rgb(value).into()
}
