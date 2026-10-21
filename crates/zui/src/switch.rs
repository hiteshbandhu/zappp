use std::{rc::Rc, time::Duration};

use gpui::{
    Animation, AnimationExt as _, App, ElementId, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, StatefulInteractiveElement, Styled, Window, div, ease_out_quint,
    prelude::FluentBuilder as _, px,
};

use crate::ActiveTheme;

const TRACK_W: f32 = 32.;
const TRACK_H: f32 = 19.;
const KNOB: f32 = 15.;
const INSET: f32 = 2.;

/// A toggle switch with an animated knob.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    on: bool,
    on_toggle: Option<Rc<dyn Fn(bool, &mut Window, &mut App)>>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, on: bool) -> Self {
        Self {
            id: id.into(),
            on,
            on_toggle: None,
        }
    }

    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Switch {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        let on = self.on;
        let off_x = INSET;
        let on_x = TRACK_W - KNOB - INSET;
        let (from, to) = if on { (off_x, on_x) } else { (on_x, off_x) };

        div()
            .id(self.id.clone())
            .relative()
            .flex_none()
            .w(px(TRACK_W))
            .h(px(TRACK_H))
            .rounded_full()
            .cursor_pointer()
            .bg(if on { t.accent } else { t.fill_selected })
            .when_some(self.on_toggle, |el, f| el.on_click(move |_, w, cx| f(!on, w, cx)))
            .child(
                div()
                    .absolute()
                    .top(px(INSET))
                    .size(px(KNOB))
                    .rounded_full()
                    .bg(gpui::white())
                    .shadow(vec![gpui::BoxShadow {
                        color: gpui::rgba(0x00000040).into(),
                        offset: gpui::point(px(0.), px(0.5)),
                        blur_radius: px(1.5),
                        spread_radius: px(0.),
                    }])
                    .with_animation(
                        ElementId::NamedChild(Box::new(self.id), if on { "on" } else { "off" }.into()),
                        Animation::new(Duration::from_millis(160)).with_easing(ease_out_quint()),
                        move |el, delta| el.left(px(from + (to - from) * delta)),
                    ),
            )
    }
}
