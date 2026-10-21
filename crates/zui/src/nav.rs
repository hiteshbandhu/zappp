use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme, Icon, IconName};

/// A source-list row, sized and colored like Finder's and System Settings'.
#[derive(IntoElement)]
pub struct NavItem {
    id: ElementId,
    icon: IconName,
    label: SharedString,
    active: bool,
    trailing: Option<SharedString>,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl NavItem {
    pub fn new(id: impl Into<ElementId>, icon: IconName, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            active: false,
            trailing: None,
            on_click: None,
        }
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn trailing(mut self, text: impl Into<SharedString>) -> Self {
        self.trailing = Some(text.into());
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for NavItem {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        div()
            .id(self.id)
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .px(px(8.))
            .rounded(t.radius)
            .text_size(px(13.))
            .text_color(t.text)
            .when(self.active, |el| el.bg(t.fill_selected))
            .child(Icon::new(self.icon).size(px(15.)).color(t.accent))
            .child(div().flex_1().child(self.label))
            .when_some(self.trailing, |el, text| {
                el.child(div().text_size(px(12.)).text_color(t.text_tertiary).child(text))
            })
            .when_some(self.on_click, |el, f| el.on_click(move |e, w, cx| f(e, w, cx)))
    }
}
