//! The persistent player bar. Until the playback controller exists it shows the empty state;
//! the player feature replaces this with `ui::player`.

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Role, Window, div, px};

pub const PLAYER_HEIGHT: f32 = 104.;

#[derive(IntoElement)]
pub struct PlayerBar;

impl RenderOnce for PlayerBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let transport = [
            ("player-shuffle", "Shuffle", IconName::Shuffle),
            ("player-previous", "Previous", IconName::SkipBack),
            ("player-play", "Play", IconName::Play),
            ("player-next", "Next", IconName::SkipForward),
            ("player-repeat", "Repeat", IconName::Repeat),
        ];

        h_flex()
            .id("player")
            .role(Role::Region)
            .aria_label("Player")
            .flex_shrink_0()
            .h(px(PLAYER_HEIGHT))
            .px_6()
            .gap_4()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .flex_1()
                    .gap_3()
                    .child(div().size(px(56.)).rounded(px(8.)).bg(theme.border))
                    .child(
                        div()
                            .id("now-playing")
                            .role(Role::Label)
                            .aria_value("Nothing playing")
                            .text_color(theme.muted_foreground)
                            .child("Nothing playing"),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .children(transport.map(|(id, label, icon)| {
                        Button::new(id)
                            .ghost()
                            .large()
                            .icon(icon)
                            .accessibility_label(label)
                            .disabled(true)
                    })),
            )
            .child(div().flex_1())
    }
}
