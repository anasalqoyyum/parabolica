//! Settings > Playback: what the OS media controls and notifications may show.

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Role, Window, div, px};

use crate::controllers::os_events;

#[derive(IntoElement)]
pub struct SystemMediaSettings;

impl RenderOnce for SystemMediaSettings {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        v_flex()
            .gap_4()
            .child(
                div()
                    .id("system-media-heading")
                    .role(Role::Heading)
                    .aria_level(2)
                    .aria_label("System media controls")
                    .text_size(px(22.))
                    .font_semibold()
                    .child("System media controls"),
            )
            .child(
                Checkbox::new("system-media-details")
                    .label("Show title, artist and artwork")
                    .checked(os_events::share_details(cx))
                    .on_click(|checked, _, cx| os_events::set_share_details(*checked, cx)),
            )
            .when_some(os_events::save_error(cx), |this, error| {
                this.child(
                    div()
                        .id("system-media-save-error")
                        .role(Role::Status)
                        .aria_label(error.clone())
                        .text_color(cx.theme().warning)
                        .child(error),
                )
            })
    }
}
