use gpui::{Context, IntoElement, ParentElement, Styled, Window, div, prelude::FluentBuilder as _, px};
use zui::{
    ActiveTheme, Button, ButtonSize, Divider, Segmented, group, group_header, h_stack, v_stack,
};

use super::{Page, Shell, footnote, row};
use crate::store::{AppCategory, CleanupEngine, Tone};

const EXAMPLES: [(Tone, &str); 4] = [
    (Tone::Verbatim, "um so the deploy is at 3, no 4pm"),
    (Tone::Clean, "So the deploy is at 4 PM."),
    (Tone::Casual, "deploy's at 4pm"),
    (Tone::Formal, "The deployment is scheduled for 4:00 PM."),
];

impl Shell {
    pub(crate) fn render_styles(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let settings = self.model.read(cx).settings.clone();
        let local = settings.cleanup == CleanupEngine::Local;

        let rows = AppCategory::ALL.iter().enumerate().map(|(ix, &cat)| {
            let model = self.model.clone();
            let selected = Tone::ALL.iter().position(|t| *t == settings.tone_for(cat)).unwrap_or(1);
            v_stack()
                .when(ix > 0, |el| el.child(Divider::inset()))
                .child(row(
                    cat.label(),
                    Some(cat.examples().into()),
                    Segmented::new(
                        gpui::ElementId::NamedInteger("tone".into(), ix as u64),
                        Tone::ALL.iter().map(|t| t.label()),
                    )
                    .selected(selected)
                    .on_select(move |i, _, cx| {
                        model.update(cx, |m, cx| {
                            m.update_settings(cx, |s| {
                                s.tones.insert(cat, Tone::ALL[i]);
                            })
                        })
                    }),
                    cx,
                ))
        });

        let examples = EXAMPLES.iter().enumerate().map(|(ix, (tone, text))| {
            v_stack().when(ix > 0, |el| el.child(Divider::inset())).child(
                h_stack()
                    .min_h(px(36.))
                    .px(px(14.))
                    .child(div().w(px(84.)).flex_none().text_color(t.text_secondary).child(tone.label()))
                    .child(div().flex_1().child(*text)),
            )
        });

        v_stack()
            .child(group_header("Writing style by app", cx))
            .child(group(cx).children(rows))
            .child(footnote(
                "zappp checks which app you're dictating into and writes the way you would there.",
                cx,
            ))
            .when(local, |el| {
                el.child(div().pt(px(12.)).child(group(cx).child(row(
                    "Casual and Formal need AI clean-up",
                    Some("On-device clean-up removes filler words. Claude rewrites tone and formats lists.".into()),
                    Button::new("styles-ai", "Settings…")
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::Settings, cx))),
                    cx,
                ))))
            })
            .child(group_header("Example", cx))
            .child(group(cx).children(examples))
    }
}
