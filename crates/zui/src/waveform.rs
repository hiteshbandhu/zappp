use gpui::{App, Hsla, IntoElement, ParentElement, RenderOnce, Styled, Window, div, px};

/// Voice-reactive bars. `levels` are 0..=1 per bar; the caller decides the
/// shape (the pill mirrors recent levels outward from the center).
#[derive(IntoElement)]
pub struct Waveform {
    levels: Vec<f32>,
    color: Hsla,
    max_height: f32,
    bar_width: f32,
    gap: f32,
}

impl Waveform {
    pub fn new(levels: Vec<f32>, color: impl Into<Hsla>) -> Self {
        Self {
            levels,
            color: color.into(),
            max_height: 16.,
            bar_width: 2.,
            gap: 2.,
        }
    }

    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = h;
        self
    }

    pub fn bar_width(mut self, w: f32) -> Self {
        self.bar_width = w;
        self
    }

    pub fn gap(mut self, g: f32) -> Self {
        self.gap = g;
        self
    }
}

impl RenderOnce for Waveform {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(self.gap))
            .h(px(self.max_height))
            .children(self.levels.iter().map(|level| {
                let level = level.clamp(0.0, 1.0);
                let h = self.bar_width + level * (self.max_height - self.bar_width);
                div()
                    .w(px(self.bar_width))
                    .h(px(h))
                    .rounded_full()
                    .bg(self.color)
            }))
    }
}
