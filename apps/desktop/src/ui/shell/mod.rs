//! Main window layout.

use gpui_kit::*;

#[derive(Debug)]
pub struct Shell;

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}
