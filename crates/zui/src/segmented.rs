use std::rc::Rc;

use gpui::{
    App, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _,
    px,
};

use crate::ActiveTheme;

/// iOS-style segmented control.
#[derive(IntoElement)]
pub struct Segmented {
    id: ElementId,
    options: Vec<SharedString>,
    selected: usize,
    on_select: Option<Rc<dyn Fn(usize, &mut Window, &mut App)>>,
}

impl Segmented {
    pub fn new(
        id: impl Into<ElementId>,
        options: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            id: id.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected: 0,
            on_select: None,
        }
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Segmented {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        let id = self.id.clone();
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .p(px(2.))
            .rounded(t.radius)
            .bg(t.fill_subtle)
            .children(self.options.into_iter().enumerate().map(|(ix, label)| {
                let selected = ix == self.selected;
                let on_select = self.on_select.clone();
                div()
                    .id(ElementId::NamedChild(Box::new(id.clone()), ix.to_string().into()))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(20.))
                    .px(px(10.))
                    .rounded(t.radius_sm)
                    .text_size(px(12.))
                    .text_color(t.text)
                    .when(selected, |el| {
                        el.bg(if t.dark { gpui::rgba(0xffffff33).into() } else { t.control })
                            .shadow(t.shadow_control())
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when_some(on_select, |el, f| el.on_click(move |_, w, cx| f(ix, w, cx)))
                    .child(label)
            }))
    }
}
