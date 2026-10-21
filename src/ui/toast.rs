//! Transient feedback in the main window: copy buttons that briefly turn
//! into a check mark, and an "Undo" toast after anything is deleted.

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, App, Context, ElementId, IntoElement,
    ParentElement, SharedString, Styled, div, px,
};
use zui::{ActiveTheme, Button, ButtonSize, IconName, h_stack};

use super::Shell;

/// How long a copy button shows its check mark.
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// How long the Undo toast stays up.
const TOAST_FOR: Duration = Duration::from_secs(6);

pub(crate) struct Toast {
    id: u64,
    message: SharedString,
    undo: Option<Box<dyn FnOnce(&mut App)>>,
    /// Runs when the toast goes away without Undo (e.g. actually deleting
    /// files that were only hidden).
    commit: Option<Box<dyn FnOnce(&mut App)>>,
}

impl Shell {
    /// A copy button that turns into a check mark for a moment after use.
    pub(crate) fn copy_button(&self, id: ElementId, text: String, cx: &mut Context<Self>) -> Button {
        let copied = self.copied.as_ref() == Some(&id);
        let key = id.clone();
        Button::icon_only(id, if copied { IconName::Check } else { IconName::Copy })
            .size(ButtonSize::Sm)
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
                this.copied = Some(key.clone());
                cx.notify();
                let key = key.clone();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(COPIED_FOR).await;
                    this.update(cx, |this, cx| {
                        if this.copied.as_ref() == Some(&key) {
                            this.copied = None;
                            cx.notify();
                        }
                    })
                    .ok();
                })
                .detach();
            }))
    }

    /// Shows "`message` · Undo" at the bottom of the window. A toast that
    /// was already showing is finished first (its deletion becomes final).
    pub(crate) fn show_toast(
        &mut self,
        message: impl Into<SharedString>,
        undo: impl FnOnce(&mut App) + 'static,
        commit: Option<Box<dyn FnOnce(&mut App)>>,
        cx: &mut Context<Self>,
    ) {
        self.finish_toast(cx);
        self.toast_seq += 1;
        let id = self.toast_seq;
        self.toast = Some(Toast {
            id,
            message: message.into(),
            undo: Some(Box::new(undo)),
            commit,
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                if this.toast.as_ref().is_some_and(|t| t.id == id) {
                    this.finish_toast(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Dismisses the toast, making its action final.
    fn finish_toast(&mut self, cx: &mut Context<Self>) {
        if let Some(commit) = self.toast.take().and_then(|t| t.commit) {
            commit(cx);
        }
        cx.notify();
    }

    fn undo_toast(&mut self, cx: &mut Context<Self>) {
        if let Some(undo) = self.toast.take().and_then(|t| t.undo) {
            undo(cx);
        }
        cx.notify();
    }

    pub(crate) fn render_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let toast = self.toast.as_ref()?;
        let t = cx.theme().clone();
        Some(
            div()
                .absolute()
                .bottom(px(22.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    h_stack()
                        .gap(px(14.))
                        .pl(px(16.))
                        .pr(px(6.))
                        .py(px(6.))
                        .rounded(px(10.))
                        .bg(t.group)
                        .border_1()
                        .border_color(t.separator)
                        .shadow_lg()
                        .child(div().text_size(px(13.)).child(toast.message.clone()))
                        .child(
                            Button::new(("toast-undo", toast.id), "Undo")
                                .size(ButtonSize::Sm)
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.undo_toast(cx))),
                        )
                        .child(
                            Button::icon_only(("toast-close", toast.id), IconName::X)
                                .size(ButtonSize::Sm)
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| this.finish_toast(cx))),
                        )
                        .with_animation(
                            ("toast-in", toast.id),
                            Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                            |el, d| el.opacity(d).mt(px(10. * (1. - d))),
                        ),
                )
                .into_any_element(),
        )
    }
}
