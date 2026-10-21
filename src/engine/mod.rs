pub mod asr;
pub mod bark;
pub mod audio;
pub mod cleanup;
pub mod coreml;
pub mod hf;
pub mod hotkey;
pub mod parakeet_coreml;
pub mod system;
pub mod tray;

use std::time::Instant;

use anyhow::Result;
use gpui::Window;
use objc2::{msg_send, runtime::AnyObject};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::store::{AppCategory, CleanupEngine, Settings, Tone};
use asr::Asr;
use system::FrontApp;

/// Everything the background pipeline needs for one dictation.
pub struct Job {
    pub samples: Vec<f32>,
    pub command: bool,
    pub selection: Option<String>,
    pub app: FrontApp,
    pub settings: Settings,
}

pub struct Outcome {
    pub raw: String,
    pub text: String,
    pub press_enter: bool,
    pub tone: Tone,
    pub audio_ms: u64,
    pub asr_ms: u64,
    pub llm_ms: Option<u64>,
    /// Non-fatal problem worth surfacing (e.g. Claude unreachable, fell back).
    pub warning: Option<String>,
}

/// Utterances this short skip the LLM entirely ("yes", "sounds good").
const FAST_PATH_WORDS: usize = 3;

/// Runs ASR + clean-up. `Ok(None)` means nothing was said.
pub fn process(asr: &Asr, job: Job) -> Result<Option<Outcome>> {
    let audio_ms = job.samples.len() as u64 * 1000 / audio::TARGET_RATE as u64;
    let mut samples = job.samples;
    audio::normalize(&mut samples);
    let speech = audio::trim_silence(&samples);
    if speech.len() < (audio::TARGET_RATE as usize / 5) {
        return Ok(None);
    }

    let t = Instant::now();
    let raw = asr.transcribe(speech)?;
    let asr_ms = t.elapsed().as_millis() as u64;
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let s = &job.settings;
    let category = AppCategory::for_bundle(&job.app.bundle_id);
    let tone = s.tone_for(category);
    let mut warning = None;
    let mut llm_ms = None;

    if job.command {
        let Some(key) = s.api_key() else {
            anyhow::bail!("Command Mode needs a Claude API key (Settings → AI)");
        };
        let t = Instant::now();
        let text = cleanup::claude_command(&key, &raw, job.selection.as_deref(), &job.app.name)?;
        return Ok(Some(Outcome {
            raw,
            text,
            press_enter: false,
            tone,
            audio_ms,
            asr_ms,
            llm_ms: Some(t.elapsed().as_millis() as u64),
            warning: None,
        }));
    }

    let (mut text, press_enter) = if s.press_enter_command {
        cleanup::take_enter_command(&raw)
    } else {
        (raw.clone(), false)
    };
    if tone != Tone::Verbatim && s.remove_fillers {
        text = cleanup::remove_fillers(&text);
    }
    text = cleanup::apply_replacements(&text, &s.replacements);
    text = cleanup::apply_vocabulary(&text, &s.vocabulary);

    let words = text.split_whitespace().count();
    let use_claude = s.cleanup == CleanupEngine::Claude && tone != Tone::Verbatim && words > FAST_PATH_WORDS;
    let key = s.api_key();
    if use_claude && let Some(key) = key {
        let t = Instant::now();
        let ctx = cleanup::Context {
            app_name: &job.app.name,
            tone,
            vocabulary: &s.vocabulary,
        };
        match cleanup::claude_format(&key, &text, &ctx) {
            Ok(polished) => {
                text = cleanup::apply_replacements(&polished, &s.replacements);
                llm_ms = Some(t.elapsed().as_millis() as u64);
            }
            Err(err) => {
                log::warn!("claude cleanup failed: {err:#}");
                warning = Some(format!("{err}"));
                text = cleanup::local_tone(&text, tone);
            }
        }
    } else {
        if use_claude {
            warning = Some("Add a Claude API key to use AI clean-up".into());
        }
        text = cleanup::local_tone(&text, tone);
    }

    Ok(Some(Outcome {
        raw,
        text,
        press_enter,
        tone,
        audio_ms,
        asr_ms,
        llm_ms,
        warning,
    }))
}

fn ns_window(window: &Window) -> Option<*mut AnyObject> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    let view = handle.ns_view.as_ptr() as *mut AnyObject;
    let window: *mut AnyObject = unsafe { msg_send![view, window] };
    (!window.is_null()).then_some(window)
}

/// The AppKit window behind a gpui window. Call its methods *outside* any
/// gpui window update: AppKit calls back into gpui synchronously when a
/// window is shown or hidden, and gpui's window state is borrowed during
/// an update ("RefCell already borrowed").
#[derive(Clone, Copy, Debug)]
pub struct NativeWindow(*mut AnyObject);

impl NativeWindow {
    pub fn of(window: &Window) -> Option<Self> {
        ns_window(window).map(Self)
    }

    /// Shows the window without activating the app, so focus stays in the
    /// app you're dictating into.
    pub fn order_front(self) {
        unsafe {
            let _: () = msg_send![self.0, orderFrontRegardless];
        }
    }

    pub fn order_out(self) {
        unsafe {
            let _: () = msg_send![self.0, orderOut: std::ptr::null_mut::<AnyObject>()];
        }
    }

    /// Makes the window a pure overlay: clicks pass through and AppKit draws
    /// no window shadow (which would outline the transparent frame).
    pub fn make_overlay(self) {
        unsafe {
            let _: () = msg_send![self.0, setIgnoresMouseEvents: true];
            let _: () = msg_send![self.0, setHasShadow: false];
            // gpui creates title-less windows as titled + full-size content,
            // which draws a 1px top highlight. Overlays should be borderless
            // (keeping only the non-activating panel bit).
            const NON_ACTIVATING_PANEL: usize = 1 << 7;
            let _: () = msg_send![self.0, setStyleMask: NON_ACTIVATING_PANEL];
        }
    }

    /// Places the window at the top center of the menu-bar screen, above the
    /// menu bar (where the notch is), on every Space and over full-screen apps.
    pub fn pin_to_notch(self, width: f64, height: f64) {
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return;
        };
        let screens = objc2_app_kit::NSScreen::screens(mtm);
        let Some(screen) = screens.firstObject() else {
            return;
        };
        let frame = screen.frame();
        let rect = NSRect::new(
            NSPoint::new(
                frame.origin.x + (frame.size.width - width) / 2.,
                frame.origin.y + frame.size.height - height,
            ),
            NSSize::new(width, height),
        );
        // Above the menu bar (24) and status items (25): pop-up menu level.
        const POP_UP_MENU_LEVEL: isize = 101;
        // canJoinAllSpaces | stationary | ignoresCycle | fullScreenAuxiliary
        const BEHAVIOR: usize = 1 | 16 | 64 | 256;
        unsafe {
            let _: () = msg_send![self.0, setLevel: POP_UP_MENU_LEVEL];
            let _: () = msg_send![self.0, setCollectionBehavior: BEHAVIOR];
            let _: () = msg_send![self.0, setFrame: rect, display: true];
        }
    }
}
