use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    Window, div, prelude::FluentBuilder as _, px,
};
use zui::{
    ActiveTheme, Button, ButtonSize, Divider, Icon, IconName, group, group_header, h_stack, v_stack,
};

use super::{Shell, element_id, footnote};

impl Shell {
    pub(crate) fn render_dictionary(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let settings = self.model.read(cx).settings.clone();

        let chips = h_stack()
            .flex_wrap()
            .gap(px(6.))
            .px(px(14.))
            .py(px(10.))
            .when(settings.vocabulary.is_empty(), |el| {
                el.child(div().text_color(t.text_tertiary).child("No words yet"))
            })
            .children(settings.vocabulary.iter().enumerate().map(|(ix, word)| {
                let model = self.model.clone();
                h_stack()
                    .gap(px(4.))
                    .h(px(22.))
                    .pl(px(8.))
                    .pr(px(3.))
                    .rounded(px(5.))
                    .bg(t.fill_subtle)
                    .text_size(px(12.))
                    .child(word.clone())
                    .child(
                        div()
                            .id(element_id("vocab-remove", ix as u64))
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(16.))
                            .rounded(px(4.))
                            .hover(|s| s.bg(t.fill_selected))
                            .child(Icon::new(IconName::X).size(px(10.)).color(t.text_secondary))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let mut removed = None;
                                model.update(cx, |m, cx| {
                                    m.update_settings(cx, |s| {
                                        if ix < s.vocabulary.len() {
                                            removed = Some(s.vocabulary.remove(ix));
                                        }
                                    })
                                });
                                let Some(word) = removed else {
                                    return;
                                };
                                let model = model.clone();
                                this.show_toast(
                                    format!("Removed “{word}”"),
                                    move |cx| {
                                        model.update(cx, |m, cx| {
                                            m.update_settings(cx, |s| s.vocabulary.insert(ix.min(s.vocabulary.len()), word))
                                        })
                                    },
                                    None,
                                    cx,
                                );
                            })),
                    )
            }));

        let len = settings.replacements.len();
        let replacements = settings.replacements.iter().enumerate().map(|(ix, r)| {
            let model = self.model.clone();
            v_stack()
                .child(Divider::inset())
                .child(
                    h_stack()
                        .gap(px(10.))
                        .min_h(px(36.))
                        .px(px(14.))
                        .child(div().flex_1().flex_basis(px(0.)).text_color(t.text_secondary).child(r.from.clone()))
                        .child(Icon::new(IconName::ChevronRight).size(px(12.)).color(t.text_tertiary))
                        .child(div().flex_1().flex_basis(px(0.)).child(r.to.clone()))
                        .child(
                            Button::icon_only(element_id("replace-remove", ix as u64), IconName::Trash)
                                .size(ButtonSize::Sm)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let mut removed = None;
                                    model.update(cx, |m, cx| {
                                        m.update_settings(cx, |s| {
                                            if ix < s.replacements.len() {
                                                removed = Some(s.replacements.remove(ix));
                                            }
                                        })
                                    });
                                    let Some(r) = removed else {
                                        return;
                                    };
                                    let model = model.clone();
                                    this.show_toast(
                                        format!("Removed “{}”", r.from),
                                        move |cx| {
                                            model.update(cx, |m, cx| {
                                                m.update_settings(cx, |s| s.replacements.insert(ix.min(s.replacements.len()), r))
                                            })
                                        },
                                        None,
                                        cx,
                                    );
                                })),
                        ),
                )
        });

        v_stack()
            .child(group_header("Vocabulary", cx))
            .child(
                group(cx)
                    .child(
                        h_stack()
                            .gap(px(8.))
                            .p(px(10.))
                            .child(div().flex_1().child(self.vocab_input.clone()))
                            .child(
                                Button::new("vocab-add", "Add")
                                    .on_click(cx.listener(|this, _, _, cx| this.add_vocabulary(cx))),
                            ),
                    )
                    .child(Divider::inset())
                    .child(chips),
            )
            .child(footnote(
                "Names, product terms and acronyms are spelled exactly as you add them.",
                cx,
            ))
            .child(group_header("Replacements", cx))
            .child(
                group(cx)
                    .child(
                        h_stack()
                            .gap(px(8.))
                            .p(px(10.))
                            .child(div().flex_1().child(self.from_input.clone()))
                            .child(div().flex_1().child(self.to_input.clone()))
                            .child(
                                Button::new("replace-add", "Add")
                                    .on_click(cx.listener(|this, _, _, cx| this.add_replacement(cx))),
                            ),
                    )
                    .children(replacements),
            )
            .child(footnote(
                if len == 0 {
                    "Expand short phrases as you speak, like “my email” into your address."
                } else {
                    "Matches whole words and ignores capitalization."
                },
                cx,
            ))
    }
}
