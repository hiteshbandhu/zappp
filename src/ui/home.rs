use gpui::{
    AnyElement, App, Context, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::FluentBuilder as _, px,
};
use zui::{
    ActiveTheme, Badge, Button, ButtonSize, Divider, Kbd, group, group_header, h_stack,
    v_stack,
};

use super::{Page, Shell, element_id, empty_state, format_ms, relative_time, row, status_label};
use crate::{app_state::AsrStatus, engine::system};

impl Shell {
    pub(crate) fn render_home(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .child(self.render_intro(cx))
            .when_some(self.render_setup(cx), |el, setup| el.child(setup))
            .child(self.render_stats(cx))
            .child(group_header("Try it", cx))
            .child(group(cx).child(div().p(px(10.)).child(self.try_input.clone())))
            .child(self.render_recent(cx))
    }

    fn render_intro(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let m = self.model.read(cx);
        let (label, tone) = status_label(m);
        let hotkey = m.settings.hotkey;
        let command = m.settings.command_hotkey;
        group(cx).child(
            h_stack()
                .gap(px(16.))
                .p(px(16.))
                .child(
                    v_stack()
                        .flex_1()
                        .min_w(px(0.))
                        .gap(px(6.))
                        .child(
                            h_stack()
                                .gap(px(6.))
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Hold")
                                .child(Kbd::new(hotkey.short()))
                                .child("anywhere and talk"),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(t.text_secondary)
                                .child(format!(
                                    "Your words appear as you talk. Release to paste and send. Double-tap {} for hands-free. \
                                     Hold {} to edit selected text.",
                                    hotkey.label(),
                                    command.label()
                                )),
                        ),
                )
                .child(Badge::new(label).tone(tone).dot()),
        )
    }

    fn render_setup(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let m = self.model.read(cx);
        let input_ok = m.permissions.input_monitoring || m.hotkey_live;
        let ax_ok = m.permissions.accessibility;
        let model_ready = matches!(m.asr_status, AsrStatus::Ready { .. });
        if input_ok && ax_ok && model_ready {
            return None;
        }

        let spec = m.model();
        let model_detail: SharedString = match (&m.asr_status, m.download) {
            (_, Some(d)) => format!("Downloading {}… {:.0}%", spec.name, d.progress * 100.0).into(),
            (AsrStatus::Loading, _) => "Loading… The first load on the Neural Engine takes about a minute.".into(),
            (AsrStatus::Failed(err), _) => err.clone(),
            _ => format!("{} runs on this Mac. {} MB, downloaded once.", spec.name, spec.size_mb).into(),
        };
        let model_entity = self.model.clone();
        let model_control: AnyElement = if model_ready {
            done(cx)
        } else if m.download.is_none() && matches!(m.asr_status, AsrStatus::Missing | AsrStatus::Failed(_)) {
            let id = spec.id;
            Button::new("setup-download", "Download")
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| model_entity.update(cx, |m, cx| m.download_model(id, cx)))
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let allow = |id: &'static str, request: fn(), url: &'static str| {
            Button::new(id, "Allow…")
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| {
                    request();
                    cx.open_url(url);
                })
                .into_any_element()
        };
        let input_control = if input_ok {
            done(cx)
        } else {
            allow("setup-input", system::request_input_monitoring, system::SETTINGS_INPUT_MONITORING)
        };
        let ax_control = if ax_ok {
            done(cx)
        } else {
            allow("setup-ax", system::request_accessibility, system::SETTINGS_ACCESSIBILITY)
        };

        Some(
            v_stack()
                .child(group_header("Get started", cx))
                .child(
                    group(cx)
                        .child(row("Speech model", Some(model_detail), model_control, cx))
                        .child(Divider::inset())
                        .child(row(
                            "Input Monitoring",
                            Some("Lets zappp notice when you hold your shortcut.".into()),
                            input_control,
                            cx,
                        ))
                        .child(Divider::inset())
                        .child(row(
                            "Accessibility",
                            Some("Needed to paste into other apps and read selected text.".into()),
                            ax_control,
                            cx,
                        )),
                )
                .into_any_element(),
        )
    }

    fn render_stats(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let separator = cx.theme().separator;
        let m = self.model.read(cx);
        let saved = m.minutes_saved();
        let stats = [
            (group_thousands(m.total_words()), "Words"),
            (
                if saved >= 60.0 { format!("{:.1} h", saved / 60.0) } else { format!("{saved:.0} min") },
                "Time saved",
            ),
            (m.history.len().to_string(), "Dictations"),
            (m.median_latency_ms().map(format_ms).unwrap_or_else(|| "–".into()), "Release to text"),
        ];
        let len = stats.len();
        let cells: Vec<_> = stats
            .into_iter()
            .enumerate()
            .map(|(ix, (value, label))| {
                h_stack()
                    .flex_1()
                    .flex_basis(px(0.))
                    .child(stat(value, label, cx))
                    .when(ix + 1 < len, |el| el.child(div().w(px(1.)).h(px(32.)).bg(separator)))
            })
            .collect();
        v_stack()
            .child(group_header("Activity", cx))
            .child(group(cx).child(h_stack().py(px(12.)).children(cells)))
    }

    fn render_recent(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let entries: Vec<_> = self.model.read(cx).history.iter().take(5).cloned().collect();
        let len = entries.len();
        v_stack()
            .child(
                h_stack()
                    .justify_between()
                    .items_end()
                    .child(group_header("Recent", cx))
                    .when(len > 0, |el| {
                        el.child(
                            div().pb(px(2.)).child(
                                Button::new("recent-all", "Show All")
                                    .ghost()
                                    .size(ButtonSize::Sm)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_page(Page::History, cx))),
                            ),
                        )
                    }),
            )
            .child(group(cx).map(|el| {
                if entries.is_empty() {
                    el.child(empty_state(
                        "No dictations yet",
                        "What you dictate shows up here so you can copy it again later.",
                        cx,
                    ))
                } else {
                    el.children(entries.into_iter().enumerate().map(|(ix, e)| {
                        let text = e.text.clone();
                        let meta = format!(
                            "{} · {} · {}",
                            if e.app_name.is_empty() { "Unknown app" } else { &e.app_name },
                            relative_time(&e.at),
                            format_ms(e.total_ms)
                        );
                        v_stack()
                            .when(ix > 0, |el| el.child(Divider::inset()))
                            .child(
                                h_stack()
                                    .gap(px(12.))
                                    .px(px(14.))
                                    .py(px(9.))
                                    .child(
                                        v_stack()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .gap(px(2.))
                                            .child(div().line_height(px(18.)).line_clamp(2).child(e.text.clone()))
                                            .child(div().text_size(px(11.5)).text_color(t.text_secondary).child(meta)),
                                    )
                                    .child(
                                        self.copy_button(element_id("recent-copy", e.id), text.clone(), cx),
                                    ),
                            )
                    }))
                }
            }))
    }
}

fn done(cx: &App) -> AnyElement {
    div()
        .text_size(px(12.))
        .text_color(cx.theme().text_secondary)
        .child("Done")
        .into_any_element()
}

fn stat(value: String, label: &'static str, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    v_stack()
        .flex_1()
        .items_center()
        .gap(px(2.))
        .child(div().text_size(px(20.)).font_weight(FontWeight::SEMIBOLD).child(value))
        .child(div().text_size(px(11.5)).text_color(t.text_secondary).child(label))
}

fn group_thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
