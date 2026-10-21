use gpui::{
    App, Div, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window,
    div, px,
};

use crate::ActiveTheme;

/// A grouped list container, like the rounded sections in System Settings.
/// Rows go inside, separated by `Divider::inset()`.
pub fn group(cx: &App) -> Div {
    let t = cx.theme();
    div()
        .flex()
        .flex_col()
        .bg(t.group)
        .rounded(t.radius_lg)
        .overflow_hidden()
}

pub fn h_stack() -> Div {
    div().flex().flex_row().items_center()
}

pub fn v_stack() -> Div {
    div().flex().flex_col()
}

/// Heading above a group, in the system's small semibold style.
pub fn group_header(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px(px(10.))
        .pt(px(18.))
        .pb(px(6.))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().text)
        .child(label.into())
}

#[derive(IntoElement)]
pub struct Divider {
    vertical: bool,
    inset: f32,
}

impl Divider {
    pub fn horizontal() -> Self {
        Self { vertical: false, inset: 0. }
    }

    /// Hairline that starts at the row's text, like native grouped lists.
    pub fn inset() -> Self {
        Self { vertical: false, inset: 14. }
    }

    pub fn vertical() -> Self {
        Self { vertical: true, inset: 0. }
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = cx.theme().separator;
        if self.vertical {
            div().w(px(1.)).h_full().bg(color)
        } else {
            div().px(px(self.inset)).child(div().h(px(1.)).w_full().bg(color))
        }
    }
}
