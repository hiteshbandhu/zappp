use gpui::{
    App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

/// Quiet status text with an optional colored dot — how macOS shows state
/// ("● On", "● Not allowed") instead of loud pills.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    tone: Tone,
    dot: bool,
    filled: bool,
}

impl Badge {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: Tone::Neutral,
            dot: false,
            filled: false,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn dot(mut self) -> Self {
        self.dot = true;
        self
    }

    /// Small rounded fill, for tags inside rows.
    pub fn filled(mut self) -> Self {
        self.filled = true;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();
        let color = match self.tone {
            Tone::Neutral => t.text_tertiary,
            Tone::Accent => t.accent,
            Tone::Success => t.green,
            Tone::Warning => t.orange,
            Tone::Danger => t.red,
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(t.text_secondary)
            .when(self.filled, |el| {
                el.h(px(18.))
                    .px(px(6.))
                    .rounded(px(4.))
                    .bg(t.fill_subtle)
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
            })
            .when(self.dot, |el| el.child(div().size(px(7.)).rounded_full().bg(color)))
            .child(self.label)
    }
}
