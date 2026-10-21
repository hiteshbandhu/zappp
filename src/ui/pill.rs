//! The dictation indicator, drawn as a black tab hanging from the camera
//! notch (or from the top edge of the menu bar on Macs without one).
//!
//! Idle it's tucked into the notch and invisible. While you talk, wings grow
//! out on both sides — a recording dot on the left, voice-reactive bars on
//! the right — and once you've said something it grows downward to show
//! your words under the notch. Size changes follow a critically damped
//! spring so they stay smooth when interrupted.

use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationExt as _, Context, Entity, FontWeight, Hsla, IntoElement, ParentElement,
    Render, SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder as _, px,
};
use zui::{ActiveTheme, Icon, IconName, Waveform, h_stack, v_stack};

use crate::{
    app_state::{AppModel, Phase},
    engine::system::Notch,
};

/// The overlay window: wide and tall enough for the expanded tab.
pub const PILL_WIDTH: f32 = 640.;
pub const PILL_HEIGHT: f32 = 120.;

/// Width of each wing beside the notch while recording.
const WING: f32 = 46.;
/// Height of the text row under the notch.
const TEXT_ROW: f32 = 30.;
const MAX_TEXT_WIDTH: f32 = 520.;
/// Rough advance of the 13 px system font, for sizing the tab to its text.
const CHAR_WIDTH: f32 = 6.9;
/// Spring stiffness: higher settles faster. ~180 ms to settle.
const SPRING: f32 = 22.;

pub struct Pill {
    model: Entity<AppModel>,
    notch: Notch,
    size: (f32, f32),
    /// Text shown last frame and how much of it was already on screen, so
    /// only newly heard words fade in.
    shown: SharedString,
    settled_len: usize,
    last_frame: Instant,
    born: Instant,
    _sub: Subscription,
}

impl Pill {
    pub fn new(model: Entity<AppModel>, notch: Notch, cx: &mut Context<Self>) -> Self {
        let sub = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            notch,
            size: (notch.width, notch.height),
            shown: SharedString::default(),
            settled_len: 0,
            last_frame: Instant::now(),
            born: Instant::now(),
            _sub: sub,
        }
    }

    /// The tab tucked away inside the notch.
    fn hidden(&self) -> (f32, f32) {
        (self.notch.width, self.notch.height)
    }

    /// The tab with its wings out.
    fn winged(&self) -> (f32, f32) {
        (self.notch.width + 2. * WING, self.notch.height)
    }

    /// The tab grown downward to hold a line of `chars` characters.
    fn with_row(&self, chars: usize) -> (f32, f32) {
        let text_width = chars as f32 * CHAR_WIDTH + 48.;
        (
            text_width.clamp(self.winged().0, MAX_TEXT_WIDTH + 48.),
            self.notch.height + TEXT_ROW,
        )
    }
}

impl Render for Pill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme().clone();
        let m = self.model.read(cx);
        let phase = m.phase.clone();
        let history: Vec<f32> = m.levels.iter().rev().copied().collect();
        let live = tail_that_fits(&m.live_text);
        if live != self.shown {
            self.settled_len = common_word_prefix(&self.shown, &live);
            self.shown = live.clone();
        }
        let has_text = !live.is_empty() && matches!(phase, Phase::Recording { .. } | Phase::Processing { .. });

        let target = match &phase {
            Phase::Idle | Phase::Done { .. } => self.hidden(),
            Phase::Error(msg) => self.with_row(msg.chars().count() + 3),
            _ if has_text => self.with_row(live.chars().count()),
            _ => self.winged(),
        };

        // Critically damped approach toward the target size.
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;
        let k = 1. - (-dt * SPRING).exp();
        self.size.0 += (target.0 - self.size.0) * k;
        self.size.1 += (target.1 - self.size.1) * k;
        let settling = (target.0 - self.size.0).abs() > 0.3 || (target.1 - self.size.1).abs() > 0.3;
        let thinking_now = matches!(phase, Phase::Processing { .. });
        if settling || thinking_now {
            window.request_animation_frame();
        }

        // Wings fade in over the last part of the expansion; the text row
        // fades in as the tab grows downward.
        let wing_open = ((self.size.0 - self.hidden().0) / (2. * WING)).clamp(0., 1.);
        let wing_opacity = ((wing_open - 0.5) / 0.5).clamp(0., 1.);
        let row_open = ((self.size.1 - self.notch.height) / TEXT_ROW).clamp(0., 1.);
        let row_opacity = ((row_open - 0.4) / 0.6).clamp(0., 1.);

        let secs = (now - self.born).as_secs_f32();
        let thinking: Vec<f32> = (0..5)
            .map(|i| 0.15 + 0.3 * ((secs * 7.0 - i as f32 * 0.7).sin() * 0.5 + 0.5))
            .collect();
        let accent = t.accent;
        let (dot, bars, text_opacity): (Option<Hsla>, Vec<f32>, f32) = match &phase {
            Phase::Recording { command: true, .. } => (Some(accent), compact(&history), 1.0),
            Phase::Recording { .. } => (Some(gpui::rgb(0xff453a).into()), compact(&history), 1.0),
            Phase::Processing { .. } => (None, thinking, 0.6),
            _ => (None, vec![], 1.0),
        };

        // Top band: the left wing, the notch itself, the right wing.
        let band = h_stack()
            .h(px(self.notch.height))
            .w_full()
            .flex_none()
            .px(px(16.))
            .justify_between()
            .opacity(wing_opacity)
            .child(
                div().w(px(14.)).flex().justify_center().when_some(dot, |el, color| {
                    el.child(
                        div().size(px(8.)).rounded_full().bg(color).with_animation(
                            "recording-dot",
                            Animation::new(Duration::from_millis(1100)).repeat(),
                            |el, d| el.opacity(0.55 + 0.45 * (d * std::f32::consts::TAU).cos().abs()),
                        ),
                    )
                }),
            )
            .child(
                div().when(!bars.is_empty(), |el| {
                    el.child(
                        Waveform::new(bars, gpui::white().opacity(if thinking_now { 0.7 } else { 1. }))
                            .max_height(14.)
                            .bar_width(2.5)
                            .gap(2.),
                    )
                }),
            );

        let row = match &phase {
            Phase::Error(msg) => Some(
                h_stack()
                    .gap(px(6.))
                    .child(Icon::new(IconName::CircleAlert).size(px(13.)).color(gpui::rgb(0xff6961)))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(gpui::white().opacity(0.9))
                            .truncate()
                            .child(msg.clone()),
                    )
                    .into_any_element(),
            ),
            _ if has_text => Some(self.live_text(text_opacity).into_any_element()),
            _ => None,
        };

        // Tighter corners while tucked against the notch, rounder once it
        // has grown into a panel.
        let radius = 9. + 7. * row_open;

        div().size_full().flex().justify_center().items_start().child(
            v_stack()
                .overflow_hidden()
                .w(px(self.size.0))
                .h(px(self.size.1))
                .bg(gpui::black())
                .rounded_b(px(radius))
                .child(band)
                .when_some(row, |el, row| {
                    el.child(
                        h_stack()
                            .h(px(TEXT_ROW))
                            .flex_none()
                            .w_full()
                            .px(px(20.))
                            .pb(px(6.))
                            .justify_center()
                            .opacity(row_opacity)
                            .child(row),
                    )
                }),
        )
    }
}

impl Pill {
    /// The words you've said so far. Words that were already showing stay
    /// put; newly heard ones fade in.
    fn live_text(&self, opacity: f32) -> impl IntoElement {
        let (settled, fresh) = self.shown.split_at(self.settled_len.min(self.shown.len()));
        let fresh_key = SharedString::from(format!("live-{}", self.shown.len()));
        h_stack()
            .whitespace_nowrap()
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(gpui::white().opacity(0.94 * opacity))
            .child(settled.to_string())
            .when(!fresh.is_empty(), |el| {
                el.child(div().child(fresh.to_string()).with_animation(
                    fresh_key,
                    Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                    |el, d| el.opacity(d),
                ))
            })
    }
}

/// The end of the transcript that fits in the tab, cut at a word boundary.
fn tail_that_fits(text: &str) -> SharedString {
    let max_chars = (MAX_TEXT_WIDTH / CHAR_WIDTH) as usize;
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string().into();
    }
    let skip = text.chars().count() - (max_chars - 1);
    let tail: String = text.chars().skip(skip).collect();
    let tail = tail.split_once(' ').map(|(_, rest)| rest).unwrap_or(&tail);
    format!("…{tail}").into()
}

/// Length of the shared prefix of `a` and `b`, backed off to a word break.
fn common_word_prefix(a: &str, b: &str) -> usize {
    let mut len = 0;
    for ((i, ca), cb) in a.char_indices().zip(b.chars()) {
        if ca != cb {
            break;
        }
        len = i + ca.len_utf8();
    }
    // All of `a` is still on screen and `b` continues after a word break.
    if len == b.len() || (len == a.len() && b[len..].starts_with(' ')) {
        return len;
    }
    b[..len].rfind(' ').map(|i| i + 1).unwrap_or(0)
}

/// Five bars from the newest levels, louder in the middle.
fn compact(newest_first: &[f32]) -> Vec<f32> {
    [2usize, 1, 0, 1, 2]
        .iter()
        .map(|&d| 0.15 + newest_first.get(d * 2).copied().unwrap_or(0.) * 0.85)
        .collect()
}
