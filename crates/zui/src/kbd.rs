use gpui::{
    App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div, px,
};

use crate::ActiveTheme;

/// A key cap, e.g. `Kbd::new("fn")`.
#[derive(IntoElement)]
pub struct Kbd {
    label: SharedString,
}

impl Kbd {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self { label: label.into() }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(20.))
            .min_w(px(20.))
            .px(px(5.))
            .rounded(px(4.))
            .bg(t.fill_subtle)
            .border_1()
            .border_color(t.separator)
            .text_size(px(11.5))
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.text_secondary)
            .child(self.label)
    }
}
