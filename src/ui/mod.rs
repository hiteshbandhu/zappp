mod brand;
mod dictionary;
mod history;
mod home;
mod modes;
mod onboarding;
pub mod pill;
mod settings;
mod toast;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement, Render, SharedString, StatefulInteractiveElement as _,
    Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use zui::{
    ActiveTheme, Badge, IconName, Kbd, NavItem, TextInput, TextInputEvent, Theme,
    Tone as BadgeTone, h_stack, v_stack,
};

use crate::app_state::{AppModel, AsrStatus, Phase};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Home,
    History,
    Dictionary,
    Styles,
    Settings,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::History => "History",
            Page::Dictionary => "Dictionary",
            Page::Styles => "Styles",
            Page::Settings => "Settings",
        }
    }
}

pub struct Shell {
    pub(crate) model: Entity<AppModel>,
    pub(crate) page: Page,
    pub(crate) search: Entity<TextInput>,
    pub(crate) vocab_input: Entity<TextInput>,
    pub(crate) from_input: Entity<TextInput>,
    pub(crate) to_input: Entity<TextInput>,
    pub(crate) key_input: Entity<TextInput>,
    pub(crate) try_input: Entity<TextInput>,
    /// Set while the first-run guide is showing.
    pub(crate) onboarding: Option<onboarding::Step>,
    /// History length when the guide's "Try it" step opened.
    pub(crate) try_baseline: usize,
    /// The copy button that was just used (shows a check mark briefly).
    pub(crate) copied: Option<gpui::ElementId>,
    pub(crate) toast: Option<toast::Toast>,
    pub(crate) toast_seq: u64,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    pub fn new(model: Entity<AppModel>, show_onboarding: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let api_key = model.read(cx).settings.anthropic_api_key.clone();
        let search = cx.new(|cx| TextInput::new(cx).placeholder("Search").icon(IconName::Search));
        let vocab_input = cx.new(|cx| TextInput::new(cx).placeholder("Add a word, name or acronym"));
        let from_input = cx.new(|cx| TextInput::new(cx).placeholder("When I say"));
        let to_input = cx.new(|cx| TextInput::new(cx).placeholder("Type"));
        let key_input = cx.new(|cx| TextInput::new(cx).placeholder("sk-ant-…").masked().with_text(api_key));
        let try_input =
            cx.new(|cx| TextInput::new(cx).placeholder("Click here, then hold your shortcut and talk"));

        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, window, cx| {
                cx.set_global(Theme::for_appearance(window.appearance()));
                cx.refresh_windows();
            }),
            cx.subscribe(&search, |_, _, _: &TextInputEvent, cx| cx.notify()),
            cx.subscribe(&vocab_input, |this, _, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Submitted) {
                    this.add_vocabulary(cx);
                }
            }),
            cx.subscribe(&to_input, |this, _, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Submitted) {
                    this.add_replacement(cx);
                }
            }),
            cx.subscribe(&key_input, |this, input, ev: &TextInputEvent, cx| {
                if matches!(ev, TextInputEvent::Changed) {
                    let key = input.read(cx).text().trim().to_string();
                    this.model
                        .update(cx, |m, cx| m.update_settings(cx, |s| s.anthropic_api_key = key));
                }
            }),
        ];

        Self {
            model,
            // Dev convenience: ZAPPP_PAGE=history|dictionary|styles|settings.
            page: match std::env::var("ZAPPP_PAGE").as_deref() {
                Ok("history") => Page::History,
                Ok("dictionary") => Page::Dictionary,
                Ok("styles") => Page::Styles,
                Ok("settings") => Page::Settings,
                _ => Page::Home,
            },
            search,
            vocab_input,
            from_input,
            to_input,
            key_input,
            try_input,
            // Dev convenience: ZAPPP_ONBOARDING=welcome|permissions|model|try|done.
            onboarding: onboarding::Step::from_env().or(show_onboarding.then_some(onboarding::Step::Welcome)),
            try_baseline: 0,
            copied: None,
            toast: None,
            toast_seq: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.page = page;
        cx.notify();
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let nav = |id: &'static str, icon, page: Page, cx: &mut Context<Self>| {
            NavItem::new(id, icon, page.title())
                .active(self.page == page)
                .on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
        };

        div().h_full().w(px(216.)).flex_none().p(px(8.)).child(
            v_stack()
                .size_full()
                .rounded(px(12.))
                .bg(t.sidebar)
                .border_1()
                .border_color(t.separator)
                .child(
                    // Traffic lights sit here; the strip drags the window.
                    div()
                        .h(px(44.))
                        .w_full()
                        .flex_none()
                        .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move()),
                )
                .child(
                    h_stack()
                        .gap(px(8.))
                        .px(px(14.))
                        .pb(px(14.))
                        .child(brand::app_icon(26.))
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("zappp")),
                )
                .child(
                    v_stack()
                        .gap(px(1.))
                        .px(px(8.))
                        .child(nav("nav-home", IconName::House, Page::Home, cx))
                        .child(nav("nav-history", IconName::History, Page::History, cx))
                        .child(nav("nav-dictionary", IconName::BookA, Page::Dictionary, cx))
                        .child(nav("nav-styles", IconName::Type, Page::Styles, cx))
                        .child(nav("nav-settings", IconName::Settings, Page::Settings, cx)),
                )
                .child(div().flex_1())
                .child(self.render_status(cx)),
        )
    }

    fn render_status(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let model = self.model.read(cx);
        let (label, tone) = status_label(model);
        v_stack()
            .gap(px(6.))
            .px(px(16.))
            .py(px(14.))
            .child(Badge::new(label).tone(tone).dot())
            .child(
                h_stack()
                    .gap(px(5.))
                    .text_size(px(12.))
                    .text_color(t.text_tertiary)
                    .child("Hold")
                    .child(Kbd::new(model.settings.hotkey.label()))
                    .child("to dictate"),
            )
    }

    pub(crate) fn add_vocabulary(&mut self, cx: &mut Context<Self>) {
        let word = self.vocab_input.read(cx).text().trim().to_string();
        if word.is_empty() {
            return;
        }
        self.model.update(cx, |m, cx| {
            m.update_settings(cx, |s| {
                if !s.vocabulary.iter().any(|w| w.eq_ignore_ascii_case(&word)) {
                    s.vocabulary.push(word)
                }
            })
        });
        self.vocab_input.update(cx, |i, cx| i.set_text("", cx));
    }

    pub(crate) fn add_replacement(&mut self, cx: &mut Context<Self>) {
        let from = self.from_input.read(cx).text().trim().to_string();
        let to = self.to_input.read(cx).text().trim().to_string();
        if from.is_empty() {
            return;
        }
        self.model.update(cx, |m, cx| {
            m.update_settings(cx, |s| s.replacements.push(crate::store::Replacement { from, to }))
        });
        self.from_input.update(cx, |i, cx| i.set_text("", cx));
        self.to_input.update(cx, |i, cx| i.set_text("", cx));
    }
}

pub(crate) fn status_label(model: &AppModel) -> (&'static str, BadgeTone) {
    match (&model.phase, &model.asr_status) {
        (Phase::Recording { .. }, _) => ("Listening", BadgeTone::Accent),
        (Phase::Processing { .. }, _) => ("Transcribing", BadgeTone::Accent),
        (_, AsrStatus::Ready { .. }) if !model.hotkey_live => ("Shortcut needs permission", BadgeTone::Warning),
        (_, AsrStatus::Ready { .. }) => ("Ready", BadgeTone::Success),
        (_, AsrStatus::Loading) => ("Loading model", BadgeTone::Neutral),
        (_, AsrStatus::Missing) if model.download.is_some() => ("Downloading model", BadgeTone::Neutral),
        (_, AsrStatus::Missing) => ("Speech model needed", BadgeTone::Warning),
        (_, AsrStatus::Failed(_)) => ("Model error", BadgeTone::Danger),
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        if let Some(step) = self.onboarding {
            return self.render_onboarding(step, window, cx);
        }
        let body: AnyElement = match self.page {
            Page::Home => self.render_home(window, cx).into_any_element(),
            Page::History => self.render_history(window, cx).into_any_element(),
            Page::Dictionary => self.render_dictionary(window, cx).into_any_element(),
            Page::Styles => self.render_styles(window, cx).into_any_element(),
            Page::Settings => self.render_settings(window, cx).into_any_element(),
        };
        let toast = self.render_toast(cx);

        h_stack()
            .items_start()
            .size_full()
            .bg(t.canvas)
            .text_color(t.text)
            .text_size(px(13.))
            .child(self.render_sidebar(cx))
            .child(
                v_stack()
                    .relative()
                    .flex_1()
                    .h_full()
                    .children(toast)
                    .child(
                        // Toolbar: page title, doubles as the window drag area.
                        h_stack()
                            .h(px(52.))
                            .w_full()
                            .flex_none()
                            .px(px(20.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
                            .child(self.page.title()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("page-{:?}", self.page)))
                            .flex_1()
                            .w_full()
                            .overflow_y_scroll()
                            .child(
                                h_stack().w_full().justify_center().child(
                                    div()
                                        .w_full()
                                        .max_w(px(680.))
                                        .px(px(24.))
                                        .pt(px(4.))
                                        .pb(px(32.))
                                        .child(body),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

// ── Building blocks shared by pages ─────────────────────────────────────

/// A grouped-list row: title (+ optional detail line) on the left, a control
/// on the right.
pub(crate) fn row(
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    let t = cx.theme();
    h_stack()
        .gap(px(16.))
        .justify_between()
        .min_h(px(40.))
        .px(px(14.))
        .py(px(9.))
        .child(
            v_stack()
                .flex_1()
                .min_w(px(0.))
                .gap(px(2.))
                .child(div().text_size(px(13.)).child(title.into()))
                .when_some(detail, |el, detail| {
                    el.child(
                        div()
                            .text_size(px(11.5))
                            .line_height(px(15.))
                            .text_color(t.text_secondary)
                            .child(detail),
                    )
                }),
        )
        .child(control)
}

/// Explanatory footnote under a group, as System Settings does.
pub(crate) fn footnote(text: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .px(px(14.))
        .pt(px(6.))
        .text_size(px(11.5))
        .line_height(px(15.))
        .text_color(cx.theme().text_secondary)
        .child(text.into())
}

pub(crate) fn empty_state(
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let t = cx.theme();
    v_stack()
        .items_center()
        .gap(px(4.))
        .py(px(36.))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.text_secondary)
                .child(title.into()),
        )
        .child(
            div()
                .max_w(px(320.))
                .text_center()
                .text_size(px(12.))
                .text_color(t.text_tertiary)
                .child(body.into()),
        )
}

pub(crate) fn relative_time(at: &chrono::DateTime<chrono::Local>) -> String {
    let secs = (chrono::Local::now() - *at).num_seconds().max(0);
    match secs {
        0..=44 => "Just now".into(),
        45..=3599 => format!("{} min ago", (secs + 30) / 60),
        3600..=86_399 => format!("{} hr ago", secs / 3600),
        _ => at.format("%b %-d").to_string(),
    }
}

pub(crate) fn format_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

pub(crate) fn element_id(prefix: &str, id: u64) -> gpui::ElementId {
    gpui::ElementId::NamedInteger(SharedString::from(prefix.to_string()), id)
}
