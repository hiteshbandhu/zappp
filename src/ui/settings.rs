use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};
use zui::{
    ActiveTheme, Badge, Button, ButtonSize, Divider, Icon, IconName, Segmented, Switch, Tone as BadgeTone, group,
    group_header, h_stack, v_stack,
};

use super::{Shell, element_id, footnote, row};
use crate::{
    app_state::AsrStatus,
    engine::system,
    models::{self, ModelSpec},
    store::{CleanupEngine, Hotkey, Settings},
};

impl Shell {
    pub(crate) fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let s = m.settings.clone();
        let perms = m.permissions;
        let hotkey_live = m.hotkey_live;
        let model = self.model.clone();

        let hotkey_index = |k: Hotkey| Hotkey::ALL.iter().position(|h| *h == k).unwrap_or(0);
        let toggle = |id: &'static str, on: bool, f: fn(&mut Settings, bool)| {
            let model = model.clone();
            Switch::new(id, on).on_toggle(move |v, _, cx| model.update(cx, |m, cx| m.update_settings(cx, |s| f(s, v))))
        };

        // ── Shortcuts
        let shortcuts = group(cx)
            .child(row("Dictation", Some("Hold to talk, release to send.".into()), {
                let model = model.clone();
                Segmented::new("hotkey", Hotkey::ALL.iter().map(|k| k.label()))
                    .selected(hotkey_index(s.hotkey))
                    .on_select(move |i, _, cx| {
                        model.update(cx, |m, cx| m.update_settings(cx, |s| s.hotkey = Hotkey::ALL[i]))
                    })
            }, cx))
            .child(Divider::inset())
            .child(row("Command Mode", Some("Hold while text is selected, then say how to change it.".into()), {
                let model = model.clone();
                Segmented::new("command-hotkey", Hotkey::ALL.iter().map(|k| k.label()))
                    .selected(hotkey_index(s.command_hotkey))
                    .on_select(move |i, _, cx| {
                        model.update(cx, |m, cx| m.update_settings(cx, |s| s.command_hotkey = Hotkey::ALL[i]))
                    })
            }, cx));

        // ── Speech model
        let speech = self.render_models(cx);

        // ── Clean-up
        let cleanup = group(cx)
            .child(row("Clean-up", None, {
                let model = model.clone();
                Segmented::new("cleanup", ["On Device", "Claude"])
                    .selected(match s.cleanup {
                        CleanupEngine::Local => 0,
                        CleanupEngine::Claude => 1,
                    })
                    .on_select(move |i, _, cx| {
                        model.update(cx, |m, cx| {
                            m.update_settings(cx, |s| {
                                s.cleanup = if i == 0 { CleanupEngine::Local } else { CleanupEngine::Claude }
                            })
                        })
                    })
            }, cx))
            .when(s.cleanup == CleanupEngine::Claude, |el| {
                el.child(Divider::inset()).child(row(
                    "API key",
                    None,
                    div().w(px(260.)).child(self.key_input.clone()),
                    cx,
                ))
            })
            .child(Divider::inset())
            .child(row(
                "Remove filler words",
                None,
                toggle("fillers", s.remove_fillers, |s, v| s.remove_fillers = v),
                cx,
            ))
            .child(Divider::inset())
            .child(row(
                "Send after pasting",
                Some("Presses Return when you release the key, so messages go out right away.".into()),
                toggle("auto-submit", s.auto_submit, |s, v| s.auto_submit = v),
                cx,
            ))
            .child(Divider::inset())
            .child(row(
                "“Press enter” sends",
                Some("End with “press enter” to submit what you dictated.".into()),
                toggle("enter", s.press_enter_command, |s, v| s.press_enter_command = v),
                cx,
            ));

        // ── General
        let general = group(cx)
            .child(row(
                "Welcome guide",
                Some("Walk through permissions, the speech model and your shortcut again.".into()),
                Button::new("show-onboarding", "Show Again")
                    .size(ButtonSize::Sm)
                    .on_click(cx.listener(|this, _, _, cx| this.start_onboarding(cx))),
                cx,
            ))
            .child(Divider::inset())
            .child(row(
                "Show pill when idle",
                Some("Keep a small indicator at the bottom of the screen.".into()),
                toggle("idle-pill", s.show_idle_pill, |s, v| s.show_idle_pill = v),
                cx,
            ))
            .child(Divider::inset())
            .child(row(
                "Restore clipboard",
                Some("Put back what you had copied after pasting.".into()),
                toggle("clipboard", s.restore_clipboard, |s, v| s.restore_clipboard = v),
                cx,
            ));

        // ── Permissions
        let permission = |title: &'static str, ok: bool, id: &'static str, request: fn(), url: &'static str| {
            row(
                title,
                None,
                if ok {
                    Badge::new("Allowed").tone(BadgeTone::Success).dot().into_any_element()
                } else {
                    Button::new(id, "Allow…")
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| {
                            request();
                            cx.open_url(url);
                        })
                        .into_any_element()
                },
                cx,
            )
        };
        let permissions = group(cx)
            .child(permission(
                "Input Monitoring",
                perms.input_monitoring || hotkey_live,
                "perm-input",
                system::request_input_monitoring,
                system::SETTINGS_INPUT_MONITORING,
            ))
            .child(Divider::inset())
            .child(permission(
                "Accessibility",
                perms.accessibility,
                "perm-ax",
                system::request_accessibility,
                system::SETTINGS_ACCESSIBILITY,
            ))
            .child(Divider::inset())
            .child(row(
                "Microphone",
                None,
                match perms.microphone {
                    system::MicAccess::Granted => {
                        Badge::new("Allowed").tone(BadgeTone::Success).dot().into_any_element()
                    }
                    system::MicAccess::NotAsked => Button::new("perm-mic", "Allow…")
                        .size(ButtonSize::Sm)
                        .on_click(|_, _, _| system::request_microphone())
                        .into_any_element(),
                    system::MicAccess::Denied => Button::new("perm-mic", "Open Settings…")
                        .size(ButtonSize::Sm)
                        .on_click(|_, _, cx| cx.open_url(system::SETTINGS_MICROPHONE))
                        .into_any_element(),
                },
                cx,
            ));

        v_stack()
            .child(group_header("Keyboard Shortcuts", cx))
            .child(shortcuts)
            .child(footnote("Double-tap the dictation key for hands-free, then tap it again to finish. Press Esc to cancel.", cx))
            .child(group_header("Microphone", cx))
            .child(self.render_microphones(cx))
            .child(group_header("Speech Model", cx))
            .child(speech)
            .child(footnote(
                "Models run entirely on this Mac. The first time a Neural Engine model loads, \
                 macOS optimizes it for your chip, which takes about a minute.",
                cx,
            ))
            .child(group_header("Text", cx))
            .child(cleanup)
            .when(s.cleanup == CleanupEngine::Claude, |el| {
                el.child(footnote(
                    "Uses Claude Haiku 5.5. The key stays on this Mac; ANTHROPIC_API_KEY also works.",
                    cx,
                ))
            })
            .child(group_header("General", cx))
            .child(general)
            .child(group_header("Permissions", cx))
            .child(permissions)
            .child(group_header("About", cx))
            .child(
                group(cx).child(
                    h_stack()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(12.))
                        .child(super::brand::app_icon(44.))
                        .child(
                            v_stack()
                                .gap(px(2.))
                                .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child("zappp"))
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(cx.theme().text_secondary)
                                        .child(format!(
                                            "Version {} · Voice to text that runs on your Mac",
                                            env!("CARGO_PKG_VERSION")
                                        )),
                                ),
                        ),
                ),
            )
    }
}

impl Shell {
    fn render_models(&self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let current = m.model().id;
        let status = m.asr_status.clone();
        let download = m.download;
        let speeds: Vec<(&'static str, Option<f64>)> =
            models::MODELS.iter().map(|spec| (spec.id, m.realtime_factor(spec.id))).collect();
        let pending = m.pending_deletes.clone();

        let rows: Vec<AnyElement> = models::MODELS
            .iter()
            .enumerate()
            .map(|(ix, spec)| {
                let selected = spec.id == current;
                let installed = spec.is_installed() && !pending.contains(&spec.id);
                let downloading = download.filter(|d| d.model == spec.id);
                let control = self.model_control(spec, selected, installed, downloading.map(|d| d.progress), &status, cx);

                let mut detail = format!("{} · {} · {} · {} MB", spec.summary, spec.languages, spec.runs_on, spec.size_mb);
                if selected {
                    if let AsrStatus::Failed(err) = &status {
                        detail = err.to_string();
                    } else if let (AsrStatus::Ready { .. }, Some(r)) =
                        (&status, speeds.iter().find(|(id, _)| *id == spec.id).and_then(|(_, r)| *r))
                    {
                        detail = format!("{detail} · recently {r:.0}× real time");
                    }
                }
                v_stack()
                    .when(ix > 0, |el| el.child(Divider::inset()))
                    .child(row(spec.name, Some(detail.into()), control, cx))
                    .into_any_element()
            })
            .collect();
        group(cx).children(rows).into_any_element()
    }

    fn model_control(
        &self,
        spec: &'static ModelSpec,
        selected: bool,
        installed: bool,
        downloading: Option<f32>,
        status: &AsrStatus,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let model = self.model.clone();
        let busy = self.model.read(cx).download.is_some();
        if let Some(progress) = downloading {
            return Badge::new(format!("Downloading {:.0}%", progress * 100.0)).dot().into_any_element();
        }
        if selected {
            return match status {
                AsrStatus::Ready { .. } => Badge::new("In Use").tone(BadgeTone::Success).dot().into_any_element(),
                AsrStatus::Loading => Badge::new("Loading…").dot().into_any_element(),
                AsrStatus::Failed(_) if installed => Button::new(element_id("model-retry", spec_ix(spec)), "Try Again")
                    .size(ButtonSize::Sm)
                    .on_click(move |_, _, cx| model.update(cx, |m, cx| m.load_asr(cx)))
                    .into_any_element(),
                _ => Button::new(element_id("model-download", spec_ix(spec)), "Download")
                    .size(ButtonSize::Sm)
                    .disabled(busy)
                    .on_click(move |_, _, cx| model.update(cx, |m, cx| m.download_model(spec.id, cx)))
                    .into_any_element(),
            };
        }
        if installed {
            let delete_model = model.clone();
            return h_stack()
                .gap(px(2.))
                .child(
                    Button::new(element_id("model-use", spec_ix(spec)), "Use")
                        .size(ButtonSize::Sm)
                        .on_click(move |_, _, cx| model.update(cx, |m, cx| m.select_model(spec.id, cx))),
                )
                .child(
                    Button::icon_only(element_id("model-delete", spec_ix(spec)), IconName::Trash)
                        .size(ButtonSize::Sm)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !delete_model.update(cx, |m, cx| m.mark_model_deleted(spec.id, cx)) {
                                return;
                            }
                            let (undo_model, commit_model) = (delete_model.clone(), delete_model.clone());
                            this.show_toast(
                                format!("Deleted {}", spec.name),
                                move |cx| undo_model.update(cx, |m, cx| m.unmark_model_deleted(spec.id, cx)),
                                Some(Box::new(move |cx| commit_model.update(cx, |m, cx| m.delete_model(spec.id, cx)))),
                                cx,
                            );
                        })),
                )
                .into_any_element();
        }
        Button::new(element_id("model-get", spec_ix(spec)), "Download")
            .size(ButtonSize::Sm)
            .disabled(busy)
            .on_click(move |_, _, cx| model.update(cx, |m, cx| m.select_model(spec.id, cx)))
            .into_any_element()
    }
}

impl Shell {
    fn render_microphones(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let m = self.model.read(cx);
        let selected = m.settings.microphone.clone();
        let default_name = m.microphones.first().cloned().unwrap_or_else(|| "No microphone".into());
        let mut options: Vec<(Option<String>, String, Option<String>)> =
            vec![(None, "System Default".into(), Some(default_name))];
        options.extend(m.microphones.iter().map(|name| (Some(name.clone()), name.clone(), None)));
        // Keep a chosen device listed even while it's unplugged.
        if let Some(name) = selected.as_ref().filter(|n| !m.microphones.contains(n)) {
            options.push((Some(name.clone()), name.clone(), Some("Not connected".into())));
        }

        let rows: Vec<AnyElement> = options
            .into_iter()
            .enumerate()
            .map(|(ix, (value, label, detail))| {
                let is_selected = value == selected;
                let model = self.model.clone();
                v_stack()
                    .when(ix > 0, |el| el.child(Divider::inset()))
                    .child(
                        div()
                            .id(element_id("mic", ix as u64))
                            .hover(|s| s.bg(t.fill_subtle))
                            .on_click(move |_, _, cx| {
                                let value = value.clone();
                                model.update(cx, |m, cx| m.update_settings(cx, |s| s.microphone = value))
                            })
                            .child(row(
                                label,
                                detail.map(Into::into),
                                div().w(px(16.)).when(is_selected, |el| {
                                    el.child(Icon::new(IconName::Check).size(px(15.)).color(t.accent))
                                }),
                                cx,
                            )),
                    )
                    .into_any_element()
            })
            .collect();
        group(cx).children(rows).into_any_element()
    }
}

fn spec_ix(spec: &ModelSpec) -> u64 {
    models::MODELS.iter().position(|m| m.id == spec.id).unwrap_or(0) as u64
}
