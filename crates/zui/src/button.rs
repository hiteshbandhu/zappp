use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, FontWeight, InteractiveElement, IntoElement, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme, Icon, IconName};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    /// Accent-filled default button.
    Primary,
    /// Standard push button.
    Secondary,
    /// Borderless; shows a fill on hover.
    Ghost,
    /// Borderless, red text.
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonSize {
    Sm,
    Md,
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<IconName>,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: Some(label.into()),
            icon: None,
            variant: ButtonVariant::Secondary,
            size: ButtonSize::Md,
            disabled: false,
            on_click: None,
        }
    }

    /// A borderless square icon button.
    pub fn icon_only(id: impl Into<ElementId>, icon: IconName) -> Self {
        Self {
            label: None,
            icon: Some(icon),
            variant: ButtonVariant::Ghost,
            ..Self::new(id, "")
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme().clone();
        let icon_only = self.label.is_none();
        let (height, pad_x, icon_size) = match self.size {
            ButtonSize::Sm => (px(22.), px(9.), px(13.)),
            ButtonSize::Md => (px(26.), px(12.), px(14.)),
        };
        let fg = match self.variant {
            ButtonVariant::Primary => t.on_accent,
            ButtonVariant::Secondary => t.text,
            ButtonVariant::Ghost => t.text_secondary,
            ButtonVariant::Danger => t.red,
        };

        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(5.))
            .h(height)
            .rounded(t.radius_sm + px(1.))
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg);

        el = if icon_only { el.w(height) } else { el.px(pad_x) };

        el = match self.variant {
            ButtonVariant::Primary => el.bg(t.accent).hover(|s| s.bg(t.accent_hover)),
            ButtonVariant::Secondary => el
                .bg(t.control)
                .when(!t.dark, |el| el.border_1().border_color(t.border))
                .shadow(t.shadow_control())
                .hover(|s| s.bg(t.control_hover)),
            ButtonVariant::Ghost | ButtonVariant::Danger => el.hover(|s| s.bg(t.fill_subtle)),
        };

        if self.disabled {
            el = el.opacity(0.4);
        } else {
            el = el.active(|s| s.opacity(0.75));
            if let Some(handler) = self.on_click {
                el = el.on_click(move |e, w, cx| handler(e, w, cx));
            }
        }

        el.when_some(self.icon, |el, icon| {
            el.child(Icon::new(icon).size(icon_size).color(fg))
        })
        .when_some(self.label, |el, label| el.child(label))
    }
}
