use chrono::{Datelike as _, Local};
use gpui::{
    Context, IntoElement, ParentElement, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use zui::{ActiveTheme, Button, ButtonSize, Divider, IconName, group, group_header, h_stack, v_stack};

use super::{Shell, element_id, empty_state, format_ms};
use crate::store::HistoryEntry;

impl Shell {
    pub(crate) fn render_history(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).text().to_lowercase();
        let model = self.model.read(cx);
        let total = model.history.len();
        let entries: Vec<HistoryEntry> = model
            .history
            .iter()
            .filter(|e| {
                query.is_empty()
                    || e.text.to_lowercase().contains(&query)
                    || e.app_name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();

        let today = Local::now().date_naive();
        let mut days: Vec<(String, Vec<HistoryEntry>)> = Vec::new();
        for e in entries {
            let day = e.at.date_naive();
            let label = if day == today {
                "Today".to_string()
            } else if Some(day) == today.pred_opt() {
                "Yesterday".to_string()
            } else if day.year() == today.year() {
                e.at.format("%A, %B %-d").to_string()
            } else {
                e.at.format("%B %-d, %Y").to_string()
            };
            match days.last_mut() {
                Some((l, items)) if *l == label => items.push(e),
                _ => days.push((label, vec![e])),
            }
        }

        let model_entity = self.model.clone();
        v_stack()
            .child(
                h_stack()
                    .gap(px(8.))
                    .child(div().flex_1().child(self.search.clone()))
                    .when(total > 0, |el| {
                        el.child(
                            Button::new("history-clear", "Clear History")
                                .size(ButtonSize::Md)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let removed = model_entity.update(cx, |m, cx| m.clear_history(cx));
                                    let count = removed.len();
                                    let model = model_entity.clone();
                                    this.show_toast(
                                        format!("Cleared {count} transcript{}", if count == 1 { "" } else { "s" }),
                                        move |cx| model.update(cx, |m, cx| m.restore_history(removed, cx)),
                                        None,
                                        cx,
                                    );
                                })),
                        )
                    }),
            )
            .map(|el| {
                if days.is_empty() {
                    el.child(div().pt(px(16.)).child(group(cx).child(if query.is_empty() {
                        empty_state("No dictations yet", "Hold your shortcut in any app and start talking.", cx)
                    } else {
                        empty_state("No results", "Try a different word or app name.", cx)
                    })))
                } else {
                    el.children(days.into_iter().map(|(label, items)| {
                        let len = items.len();
                        v_stack().child(group_header(label, cx)).child(group(cx).children(
                            items.into_iter().enumerate().map(|(ix, e)| {
                                v_stack()
                                    .child(self.history_row(e, cx))
                                    .when(ix + 1 < len, |el| el.child(Divider::inset()))
                            }),
                        ))
                    }))
                }
            })
    }

    fn history_row(&self, e: HistoryEntry, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let copy_text = e.text.clone();
        let model = self.model.clone();
        let id = e.id;
        let edited = e.raw.trim() != e.text.trim() && !e.command;

        let mut meta = vec![
            if e.app_name.is_empty() { "Unknown app".to_string() } else { e.app_name.clone() },
            if e.command { "Command".into() } else { e.tone.label().to_string() },
            format!("{} of audio", format_ms(e.audio_ms)),
            format!("transcribed in {}", format_ms(e.asr_ms)),
        ];
        if let Some(ms) = e.llm_ms {
            meta.push(format!("AI {}", format_ms(ms)));
        }

        h_stack()
            .items_start()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .child(
                div()
                    .w(px(40.))
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(t.text_tertiary)
                    .child(e.at.format("%H:%M").to_string()),
            )
            .child(
                v_stack()
                    .flex_1()
                    .min_w(px(0.))
                    .gap(px(3.))
                    .child(div().line_height(px(18.)).child(e.text.clone()))
                    .when(edited, |el| {
                        el.child(
                            div()
                                .text_size(px(11.5))
                                .line_height(px(15.))
                                .text_color(t.text_tertiary)
                                .line_clamp(2)
                                .child(format!("Heard: {}", e.raw)),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(t.text_secondary)
                            .child(meta.join(" · ")),
                    ),
            )
            .child(
                h_stack()
                    .child(
                        self.copy_button(element_id("history-copy", id), copy_text.clone(), cx),
                    )
                    .child(
                        Button::icon_only(element_id("history-delete", id), IconName::Trash)
                            .size(ButtonSize::Sm)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let Some((ix, entry)) = model.update(cx, |m, cx| m.delete_entry(id, cx)) else {
                                    return;
                                };
                                let model = model.clone();
                                this.show_toast(
                                    "Transcript deleted",
                                    move |cx| model.update(cx, |m, cx| m.restore_entry(ix, entry, cx)),
                                    None,
                                    cx,
                                );
                            })),
                    ),
            )
    }
}
