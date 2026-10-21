//! First-run onboarding: welcome, permissions, speech model, try it, done.
//! Shown full-window on first launch and from Settings → "Show Again".

use gpui::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, MouseButton, ParentElement, SharedString,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use zui::{
    ActiveTheme, Badge, Button, ButtonSize, Divider, Icon, IconName, Kbd, Segmented,
    Tone as BadgeTone, group, h_stack, v_stack,
};

use super::{Shell, brand::app_icon, format_ms, row};
use crate::{
    app_state::AsrStatus,
    engine::system::{self, MicAccess},
    store::Hotkey,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Welcome,
    Permissions,
    Model,
    TryIt,
    Done,
}

impl Step {
    const ALL: [Step; 5] = [Step::Welcome, Step::Permissions, Step::Model, Step::TryIt, Step::Done];

    pub fn from_env() -> Option<Step> {
        match std::env::var("ZAPPP_ONBOARDING").ok()?.as_str() {
            "permissions" => Some(Step::Permissions),
            "model" => Some(Step::Model),
            "try" => Some(Step::TryIt),
            "done" => Some(Step::Done),
            _ => Some(Step::Welcome),
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn next(self) -> Option<Step> {
        Self::ALL.get(self.index() + 1).copied()
    }

    fn prev(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| Self::ALL[i])
    }
}

impl Shell {
    pub fn start_onboarding(&mut self, cx: &mut Context<Self>) {
        self.onboarding = Some(Step::Welcome);
        cx.notify();
    }

    fn go_to(&mut self, step: Step, cx: &mut Context<Self>) {
        if step == Step::TryIt {
            self.try_baseline = self.model.read(cx).history.len();
        }
        self.onboarding = Some(step);
        cx.notify();
    }

    fn finish_onboarding(&mut self, cx: &mut Context<Self>) {
        self.onboarding = None;
        self.model.update(cx, |m, cx| m.update_settings(cx, |s| s.onboarded = true));
        cx.notify();
    }

    pub(crate) fn render_onboarding(&mut self, step: Step, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = cx.theme().clone();
        let body = match step {
            Step::Welcome => self.welcome(cx).into_any_element(),
            Step::Permissions => self.permissions_step(cx).into_any_element(),
            Step::Model => self.model_step(cx).into_any_element(),
            Step::TryIt => self.try_step(window, cx).into_any_element(),
            Step::Done => self.done_step(cx).into_any_element(),
        };
        let (can_continue, continue_label) = match step {
            Step::Welcome => (true, "Get Started"),
            Step::Permissions => (self.model.read(cx).permissions.all_granted(), "Continue"),
            Step::Done => (true, "Start Using zappp"),
            _ => (true, "Continue"),
        };

        v_stack()
            .size_full()
            .bg(t.canvas)
            .text_color(t.text)
            .text_size(px(13.))
            .child(
                div()
                    .h(px(44.))
                    .w_full()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
            )
            .child(
                h_stack()
                    .flex_1()
                    .w_full()
                    .justify_center()
                    .items_center()
                    .pb(px(44.))
                    .child(div().w(px(520.)).child(body)),
            )
            .child(
                h_stack()
                    .flex_none()
                    .w_full()
                    .justify_between()
                    .px(px(28.))
                    .py(px(20.))
                    .border_t_1()
                    .border_color(t.separator)
                    .child(step_dots(step, cx))
                    .child(
                        h_stack()
                            .gap(px(8.))
                            .when(step == Step::Permissions && !can_continue, |el| {
                                el.child(
                                    Button::new("onboarding-skip", "Skip for Now")
                                        .ghost()
                                        .on_click(cx.listener(|this, _, _, cx| this.go_to(Step::Model, cx))),
                                )
                            })
                            .when_some(step.prev().filter(|_| step != Step::Done), |el, prev| {
                                el.child(
                                    Button::new("onboarding-back", "Back")
                                        .on_click(cx.listener(move |this, _, _, cx| this.go_to(prev, cx))),
                                )
                            })
                            .child(
                                Button::new("onboarding-continue", continue_label)
                                    .primary()
                                    .disabled(!can_continue)
                                    .on_click(cx.listener(move |this, _, _, cx| match step.next() {
                                        _ if !can_continue => {}
                                        Some(next) => this.go_to(next, cx),
                                        None => this.finish_onboarding(cx),
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        v_stack()
            .items_center()
            .gap(px(14.))
            .child(app_icon(112.))
            .child(
                div()
                    .pt(px(6.))
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child("Welcome to zappp"),
            )
            .child(
                div()
                    .max_w(px(420.))
                    .text_center()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .text_color(t.text_secondary)
                    .child(
                        "Hold a key, talk, and let go. Your words are typed into whatever app you're \
                         using, transcribed on this Mac and never uploaded.",
                    ),
            )
    }

    fn permissions_step(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let perms = self.model.read(cx).permissions;
        let granted = || Badge::new("Allowed").tone(BadgeTone::Success).dot().into_any_element();
        let mic = match perms.microphone {
            MicAccess::Granted => granted(),
            MicAccess::NotAsked => Button::new("perm-mic", "Allow…")
                .size(ButtonSize::Sm)
                .on_click(|_, _, _| system::request_microphone())
                .into_any_element(),
            MicAccess::Denied => Button::new("perm-mic", "Open Settings…")
                .size(ButtonSize::Sm)
                .on_click(|_, _, cx| cx.open_url(system::SETTINGS_MICROPHONE))
                .into_any_element(),
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
        let input = if perms.input_monitoring {
            granted()
        } else {
            allow("perm-input", system::request_input_monitoring, system::SETTINGS_INPUT_MONITORING)
        };
        let ax = if perms.accessibility {
            granted()
        } else {
            allow("perm-ax", system::request_accessibility, system::SETTINGS_ACCESSIBILITY)
        };

        v_stack()
            .gap(px(6.))
            .child(heading("Allow access", "zappp needs three permissions. Each one updates here as soon as you allow it.", cx))
            .child(
                group(cx)
                    .child(row("Microphone", Some("To hear you while you hold the key.".into()), mic, cx))
                    .child(Divider::inset())
                    .child(row(
                        "Input Monitoring",
                        Some("To notice when you hold your shortcut. zappp only watches that key.".into()),
                        input,
                        cx,
                    ))
                    .child(Divider::inset())
                    .child(row(
                        "Accessibility",
                        Some("To type the text into the app you're using.".into()),
                        ax,
                        cx,
                    )),
            )
            .child(note(
                "In System Settings, switch zappp on in the list. If zappp isn't listed, use the + button \
                 and choose zappp from where you keep it.",
                cx,
            ))
    }

    fn model_step(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let m = self.model.read(cx);
        let spec = m.model();
        let download = m.download;
        let status = m.asr_status.clone();
        let model = self.model.clone();
        let id = spec.id;

        let (title, detail, progress): (&str, SharedString, Option<f32>) = match (&status, download) {
            (_, Some(d)) => ("Downloading…", format!("{} MB, one time.", spec.size_mb).into(), Some(d.progress)),
            (AsrStatus::Loading, _) => (
                "Optimizing for this Mac…",
                "macOS is tuning the model for your Neural Engine. This takes about a minute, once.".into(),
                None,
            ),
            (AsrStatus::Ready { load_ms }, _) => ("Ready", format!("Loaded in {}.", format_ms(*load_ms)).into(), None),
            (AsrStatus::Failed(err), _) => ("Something went wrong", err.clone(), None),
            (AsrStatus::Missing, None) => ("Not downloaded", format!("{} MB, one time.", spec.size_mb).into(), None),
        };
        let control: AnyElement = match (&status, download) {
            (_, Some(_)) | (AsrStatus::Loading, _) => div().into_any_element(),
            (AsrStatus::Ready { .. }, _) => Icon::new(IconName::CircleCheck).size(px(18.)).color(t.green).into_any_element(),
            (AsrStatus::Failed(_), _) if spec.is_installed() => Button::new("model-retry", "Try Again")
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| model.update(cx, |m, cx| m.load_asr(cx)))
                .into_any_element(),
            _ => Button::new("model-download", "Download")
                .size(ButtonSize::Sm)
                .on_click(move |_, _, cx| model.update(cx, |m, cx| m.download_model(id, cx)))
                .into_any_element(),
        };

        v_stack()
            .gap(px(6.))
            .child(heading(
                "Speech model",
                "zappp transcribes on your Mac, so nothing you say leaves it.",
                cx,
            ))
            .child(
                group(cx)
                    .child(row(
                        format!("{} · {}", spec.name, spec.languages),
                        Some(format!("{title} {detail}").into()),
                        control,
                        cx,
                    ))
                    .when_some(progress, |el, p| el.child(div().px(px(14.)).pb(px(12.)).child(progress_bar(p, cx)))),
            )
            .child(note("You can keep going while this finishes. Other models are in Settings.", cx))
    }

    fn try_step(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let m = self.model.read(cx);
        let hotkey = m.settings.hotkey;
        let succeeded = m.history.len() > self.try_baseline;
        let last_ms = m.history.first().map(|e| e.total_ms);
        let selected = Hotkey::ALL.iter().position(|k| *k == hotkey).unwrap_or(0);
        let model = self.model.clone();

        v_stack()
            .gap(px(6.))
            .child(heading("Try it", "Pick your key, then hold it and talk. Let go to send.", cx))
            .child(
                group(cx)
                    .child(row(
                        "Dictation key",
                        None,
                        Segmented::new("onboarding-hotkey", Hotkey::ALL.iter().map(|k| k.label()))
                            .selected(selected)
                            .on_select(move |i, _, cx| {
                                model.update(cx, |m, cx| m.update_settings(cx, |s| s.hotkey = Hotkey::ALL[i]))
                            }),
                        cx,
                    ))
                    .child(Divider::inset())
                    .child(
                        v_stack()
                            .gap(px(8.))
                            .p(px(14.))
                            .child(
                                h_stack()
                                    .gap(px(5.))
                                    .text_color(t.text_secondary)
                                    .child("Click the field, hold")
                                    .child(Kbd::new(hotkey.label()))
                                    .child("and say something like “hello from zappp”."),
                            )
                            .child(self.try_input.clone()),
                    ),
            )
            .child(div().pt(px(6.)).child(if succeeded {
                h_stack()
                    .gap(px(8.))
                    .px(px(4.))
                    .child(Icon::new(IconName::CircleCheck).size(px(16.)).color(t.green))
                    .child(format!(
                        "That worked{}.",
                        last_ms.map(|ms| format!(" — text in {}", format_ms(ms))).unwrap_or_default()
                    ))
                    .into_any_element()
            } else {
                note("Nothing happening? Check the previous steps: the key needs Input Monitoring.", cx)
                    .into_any_element()
            }))
    }

    fn done_step(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.model.read(cx);
        let hotkey = m.settings.hotkey.label();
        let command = m.settings.command_hotkey.label();
        let tip = |keys: Vec<&'static str>, what: &'static str, cx: &App| {
            h_stack()
                .gap(px(10.))
                .min_h(px(40.))
                .px(px(14.))
                .child(h_stack().w(px(150.)).gap(px(4.)).children(keys.into_iter().map(Kbd::new)))
                .child(div().text_color(cx.theme().text_secondary).child(what))
        };
        v_stack()
            .items_center()
            .gap(px(14.))
            .child(app_icon(72.))
            .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child("You're all set"))
            .child(
                div()
                    .w_full()
                    .child(
                        group(cx)
                            .child(tip(vec![hotkey], "Hold to dictate, let go to send", cx))
                            .child(Divider::inset())
                            .child(tip(vec![hotkey, hotkey], "Double-tap for hands-free", cx))
                            .child(Divider::inset())
                            .child(tip(vec!["esc"], "Cancel while talking", cx))
                            .child(Divider::inset())
                            .child(tip(vec![command], "Hold with text selected to rewrite it", cx)),
                    ),
            )
            .child(note("zappp lives in the menu bar. Open it there any time.", cx))
    }
}

fn heading(title: &'static str, subtitle: &'static str, cx: &App) -> impl IntoElement {
    v_stack()
        .gap(px(6.))
        .pb(px(14.))
        .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child(title))
        .child(
            div()
                .text_size(px(13.5))
                .line_height(px(20.))
                .text_color(cx.theme().text_secondary)
                .child(subtitle),
        )
}

fn note(text: &'static str, cx: &App) -> gpui::Div {
    div()
        .px(px(4.))
        .pt(px(8.))
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(cx.theme().text_secondary)
        .child(text)
}

fn progress_bar(progress: f32, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(t.fill_subtle)
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(t.accent)
                .w(gpui::relative(progress.clamp(0.02, 1.))),
        )
}

fn step_dots(step: Step, cx: &App) -> impl IntoElement {
    let t = cx.theme();
    h_stack().gap(px(6.)).children(Step::ALL.iter().map(|s| {
        div()
            .size(px(6.))
            .rounded_full()
            .bg(if *s == step { t.accent } else { t.fill_selected })
    }))
}
