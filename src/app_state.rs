//! The app model: settings, history and the live dictation state machine.

use std::{
    collections::VecDeque,
    io::{Read as _, Write as _},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, anyhow};
use chrono::Local;
use gpui::{AnyWindowHandle, ClipboardItem, Context, SharedString};

use crate::{
    engine::{
        self, Job, Outcome,
        asr::Asr,
        audio::{self, Recorder},
        hotkey::{HotkeyConfig, HotkeyEvent},
        system::{self, FrontApp, Permissions},
        tray,
    },
    models::{self, Engine, ModelSpec, Source},
    store::{self, HistoryEntry, Settings},
};

pub const LEVEL_BARS: usize = 26;
/// A press shorter than this is a tap, not a dictation.
const TAP_THRESHOLD: Duration = Duration::from_millis(250);
/// The pill appears once the key has been held this long.
const SHOW_PILL_AFTER: Duration = Duration::from_millis(180);
/// Two taps this close together start hands-free dictation.
const DOUBLE_TAP: Duration = Duration::from_millis(450);
/// How often the live preview re-transcribes while you speak.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(250);
/// The preview only looks at the most recent audio; the pill shows the tail.
const PREVIEW_WINDOW_SECS: f32 = 14.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Recording { command: bool, hands_free: bool },
    Processing { command: bool },
    Done { words: usize, warning: Option<SharedString> },
    Error(SharedString),
}

/// State of the selected speech model.
#[derive(Clone, Debug, PartialEq)]
pub enum AsrStatus {
    /// Not downloaded yet (see `AppModel::download` for progress).
    Missing,
    Loading,
    Ready { load_ms: u64 },
    Failed(SharedString),
}

/// A model download in flight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Download {
    pub model: &'static str,
    pub progress: f32,
}

struct Session {
    started: Instant,
    command: bool,
    hands_free: bool,
    app: FrontApp,
    selection: Option<String>,
}

pub struct AppModel {
    pub settings: Settings,
    pub history: Vec<HistoryEntry>,
    pub phase: Phase,
    pub levels: VecDeque<f32>,
    pub asr_status: AsrStatus,
    pub download: Option<Download>,
    pub permissions: Permissions,
    pub hotkey_live: bool,
    pub pill: Option<AnyWindowHandle>,
    /// What you've said so far, transcribed live while recording.
    pub live_text: SharedString,
    /// Input devices, system default first. Refreshed in the background.
    pub microphones: Vec<String>,
    /// Models deleted in the UI whose files are removed once Undo expires.
    pub pending_deletes: Vec<&'static str>,
    last_tap: Option<Instant>,
    recorder: Arc<Recorder>,
    asr: Arc<Asr>,
    hotkeys: Arc<HotkeyConfig>,
    session: Option<Session>,
    phase_epoch: u64,
}

impl AppModel {
    pub fn new(hotkeys: Arc<HotkeyConfig>, cx: &mut Context<Self>) -> Self {
        let settings = store::load_settings();
        hotkeys.set(settings.hotkey, settings.command_hotkey);
        let mut this = Self {
            history: store::load_history(),
            settings,
            phase: Phase::Idle,
            levels: VecDeque::from(vec![0.0; LEVEL_BARS]),
            asr_status: AsrStatus::Missing,
            download: None,
            permissions: system::permissions(),
            hotkey_live: false,
            pill: None,
            live_text: SharedString::default(),
            microphones: Vec::new(),
            pending_deletes: Vec::new(),
            last_tap: None,
            recorder: Arc::new(Recorder::new()),
            asr: Arc::new(Asr::new()),
            hotkeys,
            session: None,
            phase_epoch: 0,
        };
        if this.model().is_installed() {
            this.load_asr(cx);
        }
        log::info!(
            "permissions: input monitoring {}, accessibility {}",
            this.permissions.input_monitoring,
            this.permissions.accessibility
        );
        this.poll_permissions(cx);
        this
    }

    // ── Speech model ────────────────────────────────────────────────────

    /// The model selected in settings.
    pub fn model(&self) -> &'static ModelSpec {
        models::find(&self.settings.model)
    }

    pub fn load_asr(&mut self, cx: &mut Context<Self>) {
        self.asr_status = AsrStatus::Loading;
        cx.notify();
        let asr = self.asr.clone();
        let spec = self.model();
        let task = cx.background_executor().spawn(async move { asr.load(spec) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                // Ignore a load that finished after the user switched models.
                if this.model().id != spec.id {
                    return;
                }
                this.asr_status = match result {
                    Ok(load_time) => {
                        log::info!("{} loaded in {load_time:?}", spec.id);
                        AsrStatus::Ready {
                            load_ms: load_time.as_millis() as u64,
                        }
                    }
                    Err(err) => AsrStatus::Failed(format!("{err:#}").into()),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Makes `id` the active model, downloading it first if needed.
    pub fn select_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if self.settings.model == id {
            return;
        }
        self.update_settings(cx, |s| s.model = id.to_string());
        self.asr.unload();
        if self.model().is_installed() {
            self.load_asr(cx);
        } else {
            self.asr_status = AsrStatus::Missing;
            self.download_model(id, cx);
        }
    }

    /// Whether a model is downloaded and not waiting to be deleted.
    pub fn model_available(&self, spec: &ModelSpec) -> bool {
        spec.is_installed() && !self.pending_deletes.contains(&spec.id)
    }

    /// Hides a model as deleted; its files go once the Undo toast expires
    /// (see `delete_model`). Returns false if it can't be deleted now.
    pub fn mark_model_deleted(&mut self, id: &'static str, cx: &mut Context<Self>) -> bool {
        if self.settings.model == id || self.download.is_some_and(|d| d.model == id) {
            return false;
        }
        self.pending_deletes.push(id);
        cx.notify();
        true
    }

    pub fn unmark_model_deleted(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.pending_deletes.retain(|p| *p != id);
        cx.notify();
    }

    /// Deletes a downloaded model's files (unless it was put back in use).
    pub fn delete_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        self.pending_deletes.retain(|p| *p != id);
        if self.settings.model == id || self.download.is_some_and(|d| d.model == id) {
            return;
        }
        let path = models::find(id).path();
        if let Err(err) = std::fs::remove_dir_all(&path) {
            log::warn!("could not delete {}: {err}", path.display());
        }
        cx.notify();
    }

    pub fn download_model(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if self.download.is_some() {
            return;
        }
        let spec = models::find(id);
        let progress = Arc::new(AtomicU32::new(0));
        self.download = Some(Download { model: spec.id, progress: 0.0 });
        if self.model().id == spec.id {
            self.asr_status = AsrStatus::Missing;
        }
        cx.notify();

        let task_progress = progress.clone();
        let task = cx.background_executor().spawn(async move {
            install_model(spec, |p| task_progress.store(p.to_bits(), Ordering::Relaxed))
        });
        cx.spawn(async move |this, cx| {
            let mut task = std::pin::pin!(task);
            loop {
                let tick = cx.background_executor().timer(Duration::from_millis(200));
                match futures::future::select(task.as_mut(), tick).await {
                    futures::future::Either::Left((result, _)) => {
                        this.update(cx, |this, cx| {
                            this.download = None;
                            match result {
                                Ok(()) if this.model().id == spec.id => this.load_asr(cx),
                                Ok(()) => cx.notify(),
                                Err(err) => {
                                    log::warn!("download of {} failed: {err:#}", spec.id);
                                    if this.model().id == spec.id {
                                        this.asr_status =
                                            AsrStatus::Failed(format!("Download failed: {err:#}").into());
                                    }
                                    cx.notify();
                                }
                            }
                        })
                        .ok();
                        break;
                    }
                    futures::future::Either::Right(_) => {
                        let p = f32::from_bits(progress.load(Ordering::Relaxed));
                        let alive = this
                            .update(cx, |this, cx| {
                                this.download = Some(Download { model: spec.id, progress: p });
                                cx.notify();
                            })
                            .is_ok();
                        if !alive {
                            break;
                        }
                    }
                }
            }
        })
        .detach();
    }

    // ── Permissions ─────────────────────────────────────────────────────

    /// Keeps permissions, the shortcut status and the microphone list
    /// current. The system queries (CoreAudio can be slow with virtual
    /// devices) run off the main thread.
    fn poll_permissions(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                let (perms, mics) = cx
                    .background_executor()
                    .spawn(async { (system::permissions(), audio::input_devices()) })
                    .await;
                let updated = this.update(cx, |this, cx| {
                    // The tap can exist without Input Monitoring; it just gets
                    // no key events. Only call the shortcut live with both.
                    let live = this.hotkeys.tap_running.load(Ordering::Relaxed) && perms.input_monitoring;
                    if perms != this.permissions || live != this.hotkey_live || mics != this.microphones {
                        if perms != this.permissions {
                            log::info!(
                                "permissions: input monitoring {}, accessibility {}",
                                perms.input_monitoring,
                                perms.accessibility
                            );
                        }
                        this.permissions = perms;
                        this.hotkey_live = live;
                        this.microphones = mics;
                        cx.notify();
                    }
                });
                // A busy app just means we try again next tick; only stop
                // once the model is gone.
                if updated.is_err() && this.upgrade().is_none() {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(1500)).await;
            }
        })
        .detach();
    }

    // ── Settings & history ──────────────────────────────────────────────

    pub fn update_settings(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Settings)) {
        f(&mut self.settings);
        self.hotkeys.set(self.settings.hotkey, self.settings.command_hotkey);
        store::save_settings(&self.settings);
        self.sync_pill(cx);
        cx.notify();
    }

    /// Removes a history entry, returning it and its position for Undo.
    pub fn delete_entry(&mut self, id: u64, cx: &mut Context<Self>) -> Option<(usize, HistoryEntry)> {
        let ix = self.history.iter().position(|e| e.id == id)?;
        let entry = self.history.remove(ix);
        store::save_history(&self.history);
        cx.notify();
        Some((ix, entry))
    }

    pub fn restore_entry(&mut self, ix: usize, entry: HistoryEntry, cx: &mut Context<Self>) {
        self.history.insert(ix.min(self.history.len()), entry);
        store::save_history(&self.history);
        cx.notify();
    }

    /// Clears history, returning what was there for Undo.
    pub fn clear_history(&mut self, cx: &mut Context<Self>) -> Vec<HistoryEntry> {
        let removed = std::mem::take(&mut self.history);
        store::save_history(&self.history);
        cx.notify();
        removed
    }

    pub fn restore_history(&mut self, mut entries: Vec<HistoryEntry>, cx: &mut Context<Self>) {
        // Keep anything dictated since the clear, newest first.
        entries.splice(0..0, std::mem::take(&mut self.history));
        self.history = entries;
        store::save_history(&self.history);
        cx.notify();
    }

    // ── Dictation state machine ─────────────────────────────────────────

    pub fn handle_hotkey(&mut self, event: HotkeyEvent, cx: &mut Context<Self>) {
        log::debug!("hotkey {event:?} in {:?}", self.phase);
        match event {
            HotkeyEvent::DictateDown | HotkeyEvent::CommandDown => {
                let command = event == HotkeyEvent::CommandDown;
                match &self.phase {
                    Phase::Recording { hands_free: true, .. } => self.finish(cx),
                    Phase::Recording { .. } | Phase::Processing { .. } => {}
                    _ => {
                        // Double-tap starts hands-free; otherwise it's hold-to-talk.
                        let hands_free = !command && self.last_tap.take().is_some_and(|t| t.elapsed() < DOUBLE_TAP);
                        self.start(command, hands_free, cx)
                    }
                }
            }
            HotkeyEvent::DictateUp | HotkeyEvent::CommandUp => {
                let Some(session) = self.session.as_ref() else {
                    return;
                };
                if session.hands_free {
                    return;
                }
                if session.started.elapsed() < TAP_THRESHOLD && !session.command {
                    // Too short to be speech: treat it as a tap, so a second
                    // tap right after can start hands-free mode.
                    self.cancel(cx);
                    self.last_tap = Some(Instant::now());
                } else {
                    // Releasing always finishes: transcribe, paste, submit.
                    self.finish(cx);
                }
            }
            HotkeyEvent::Escape => {
                if self.session.is_some() {
                    self.cancel(cx);
                }
            }
            HotkeyEvent::Chord => {
                // A shortcut like Right ⌘ + C, not dictation. Hands-free
                // sessions are left alone: you may be typing during them.
                if self.session.as_ref().is_some_and(|s| !s.hands_free) {
                    log::info!("key used in a shortcut; not dictating");
                    self.cancel(cx);
                }
            }
        }
    }

    /// Starts hands-free dictation into the frontmost app, or finishes the
    /// current one (menu bar "Start/Stop Dictation").
    pub fn toggle_dictation(&mut self, cx: &mut Context<Self>) {
        match &self.phase {
            Phase::Recording { .. } => self.finish(cx),
            Phase::Processing { .. } => {}
            _ => self.start(false, true, cx),
        }
    }

    /// What the menu bar menu should show right now.
    pub fn menu_state(&self) -> tray::MenuState {
        let key = self.settings.hotkey.label();
        let (label, _) = crate::ui::status_label(self);
        let status = match label {
            "Ready" => format!("Ready — hold {key} to dictate"),
            other => other.to_string(),
        };
        tray::MenuState {
            status,
            can_dictate: self.asr.is_loaded(),
            dictating: matches!(self.phase, Phase::Recording { .. }),
            recent: self.history.iter().take(5).map(|e| preview(&e.text, 48)).collect(),
            microphones: self.microphones.clone(),
            microphone: self.settings.microphone.clone(),
            models: models::MODELS
                .iter()
                .filter(|m| self.model_available(m))
                .map(|m| (m.id, m.name.to_string()))
                .collect(),
            model: self.model().id,
            auto_submit: self.settings.auto_submit,
        }
    }

    fn start(&mut self, command: bool, hands_free: bool, cx: &mut Context<Self>) {
        if !self.asr.is_loaded() {
            let msg = match &self.asr_status {
                AsrStatus::Missing if self.download.is_some() => "Speech model is still downloading",
                AsrStatus::Missing => "Download the speech model first",
                AsrStatus::Failed(_) => "Speech model failed to load",
                _ => "Speech model is still loading",
            };
            self.set_phase(Phase::Error(msg.into()), cx);
            return;
        }
        let app = system::frontmost_app();
        let selection = if command { system::selected_text() } else { None };
        log::info!("recording (command: {command}, hands-free: {hands_free}) into {:?}", app.name);
        if let Err(err) = self.recorder.start(self.settings.microphone.clone()) {
            log::error!("could not start recording: {err:#}");
            self.set_phase(Phase::Error(format!("{err:#}").into()), cx);
            return;
        }
        let started = Instant::now();
        self.session = Some(Session {
            started,
            command,
            hands_free,
            app,
            selection,
        });
        self.levels = VecDeque::from(vec![0.0; LEVEL_BARS]);
        self.live_text = SharedString::default();
        self.set_phase(Phase::Recording { command, hands_free }, cx);
        self.run_level_meter(cx);
        self.run_live_preview(started, cx);
    }

    /// Re-transcribes the recent audio while you talk so the pill can show
    /// your words as you say them. Runs one transcription at a time; with the
    /// Neural Engine each pass takes well under 100 ms.
    fn run_live_preview(&mut self, session_started: Instant, cx: &mut Context<Self>) {
        let recorder = self.recorder.clone();
        let asr = self.asr.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PREVIEW_INTERVAL).await;
                let active = this
                    .update(cx, |this, _| {
                        matches!(this.phase, Phase::Recording { .. })
                            && this.session.as_ref().is_some_and(|s| s.started == session_started)
                    })
                    .unwrap_or(false);
                if !active {
                    break;
                }
                let audio = recorder.snapshot(PREVIEW_WINDOW_SECS);
                if audio.len() < audio::TARGET_RATE as usize / 2 {
                    continue;
                }
                let asr = asr.clone();
                let text = cx
                    .background_executor()
                    .spawn(async move { asr.transcribe(&audio) })
                    .await;
                let Ok(text) = text else {
                    continue;
                };
                this.update(cx, |this, cx| {
                    let same = this.session.as_ref().is_some_and(|s| s.started == session_started);
                    if same && matches!(this.phase, Phase::Recording { .. }) && !text.is_empty() {
                        this.live_text = text.into();
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    fn run_level_meter(&mut self, cx: &mut Context<Self>) {
        let recorder = self.recorder.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        if !matches!(this.phase, Phase::Recording { .. }) {
                            return false;
                        }
                        this.levels.pop_front();
                        this.levels.push_back(recorder.level());
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.session = None;
        let _ = self.recorder.stop();
        self.set_phase(Phase::Idle, cx);
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.take() else {
            return;
        };
        let recording = self.recorder.stop();
        let released = Instant::now();
        self.set_phase(Phase::Processing { command: session.command }, cx);

        let job = Job {
            samples: recording.samples,
            command: session.command,
            selection: session.selection.clone(),
            app: session.app.clone(),
            settings: self.settings.clone(),
        };
        let asr = self.asr.clone();
        let task = cx.background_executor().spawn(async move { engine::process(&asr, job) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| this.on_processed(result, session, released, cx))
                .ok();
        })
        .detach();
    }

    fn on_processed(
        &mut self,
        result: anyhow::Result<Option<Outcome>>,
        session: Session,
        released: Instant,
        cx: &mut Context<Self>,
    ) {
        let outcome = match result {
            Ok(Some(outcome)) => outcome,
            Ok(None) => {
                log::info!("no speech detected");
                self.set_phase(Phase::Idle, cx);
                return;
            }
            Err(err) => {
                log::error!("dictation failed: {err:#}");
                self.set_phase(Phase::Error(format!("{err:#}").into()), cx);
                return;
            }
        };
        log::info!(
            "transcribed {} ms of audio in {} ms; pasting {} chars",
            outcome.audio_ms,
            outcome.asr_ms,
            outcome.text.len()
        );
        if !self.permissions.accessibility {
            log::warn!("Accessibility is not allowed; the paste keystroke will be ignored by macOS");
        }
        // Release means send: paste, then press Return (unless turned off, or
        // in Command Mode where we're replacing a selection).
        let submit = outcome.press_enter || (self.settings.auto_submit && !session.command);
        self.insert_text(&outcome.text, submit, cx);
        let total_ms = released.elapsed().as_millis() as u64;
        let entry = HistoryEntry {
            model: self.model().id.to_string(),
            id: self.history.iter().map(|e| e.id).max().unwrap_or(0) + 1,
            at: Local::now(),
            raw: outcome.raw,
            text: outcome.text,
            app_name: session.app.name,
            bundle_id: session.app.bundle_id,
            tone: outcome.tone,
            command: session.command,
            audio_ms: outcome.audio_ms,
            asr_ms: outcome.asr_ms,
            llm_ms: outcome.llm_ms,
            total_ms,
        };
        let words = entry.words();
        self.history.insert(0, entry);
        self.history.truncate(2000);
        store::save_history(&self.history);
        self.set_phase(
            Phase::Done {
                words,
                warning: outcome.warning.map(Into::into),
            },
            cx,
        );
    }

    /// Paste via the clipboard, then put the user's clipboard back.
    pub fn insert_text(&self, text: &str, press_enter: bool, cx: &mut Context<Self>) {
        let saved = self
            .settings
            .restore_clipboard
            .then(|| cx.read_from_clipboard())
            .flatten();
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        system::send_paste();
        cx.spawn(async move |_, cx| {
            if press_enter {
                cx.background_executor().timer(Duration::from_millis(90)).await;
                system::send_return();
            }
            cx.background_executor().timer(Duration::from_millis(450)).await;
            if let Some(saved) = saved {
                cx.update(|cx| cx.write_to_clipboard(saved)).ok();
            }
        })
        .detach();
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if matches!(phase, Phase::Idle | Phase::Error(_)) {
            self.live_text = SharedString::default();
        }
        self.phase = phase;
        self.phase_epoch += 1;
        let epoch = self.phase_epoch;
        tray::set_recording(matches!(self.phase, Phase::Recording { .. }));

        let linger = match &self.phase {
            Phase::Done { warning: Some(_), .. } => Some(2600),
            Phase::Done { .. } => Some(300),
            Phase::Error(_) => Some(2800),
            _ => None,
        };
        if let Some(ms) = linger {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(ms)).await;
                this.update(cx, |this, cx| {
                    if this.phase_epoch == epoch {
                        this.set_phase(Phase::Idle, cx);
                    }
                })
                .ok();
            })
            .detach();
        }

        self.sync_pill(cx);
        cx.notify();
    }

    /// Shows or hides the pill window. When it should disappear we wait for
    /// the collapse animation to finish first.
    pub fn sync_pill(&mut self, cx: &mut Context<Self>) {
        let Some(pill) = self.pill else {
            return;
        };
        let visible = self.settings.show_idle_pill || self.phase != Phase::Idle;
        let epoch = self.phase_epoch;
        // Holding the key shows the pill only once it's clearly a hold, so
        // quick presses and shortcuts never flash it.
        let wait = match (&self.phase, &self.session) {
            (Phase::Recording { hands_free: false, .. }, Some(session)) => {
                SHOW_PILL_AFTER.saturating_sub(session.started.elapsed())
            }
            _ => Duration::ZERO,
        };
        cx.spawn(async move |this, cx| {
            if visible && !wait.is_zero() {
                cx.background_executor().timer(wait).await;
                let still_current = this.update(cx, |this, _| this.phase_epoch == epoch).unwrap_or(false);
                if !still_current {
                    return;
                }
            }
            if !visible {
                cx.background_executor().timer(Duration::from_millis(240)).await;
                let still_idle = this.update(cx, |this, _| this.phase_epoch == epoch).unwrap_or(false);
                if !still_idle {
                    return;
                }
            }
            let native = cx
                .update(|cx| pill.update(cx, |_, window, _| engine::NativeWindow::of(window)).ok().flatten())
                .ok()
                .flatten();
            if let Some(native) = native {
                if visible {
                    native.order_front();
                } else {
                    native.order_out();
                }
            }
        })
        .detach();
    }

    /// Dev-only: fills history in memory and shows the pill recording with
    /// simulated levels, so the UI can be reviewed without speaking.
    pub fn start_demo(&mut self, cx: &mut Context<Self>) {
        let now = Local::now();
        let sample = |id: u64, mins: i64, text: &str, raw: &str, app: &str, tone, llm: Option<u64>, total| HistoryEntry {
            model: crate::models::DEFAULT_MODEL.to_string(),
            id,
            at: now - chrono::Duration::minutes(mins),
            raw: raw.into(),
            text: text.into(),
            app_name: app.into(),
            bundle_id: String::new(),
            tone,
            command: false,
            audio_ms: 4200,
            asr_ms: 150,
            llm_ms: llm,
            total_ms: total,
        };
        use crate::store::Tone;
        self.history = vec![
            sample(4, 2, "Sounds good, let's ship it Friday", "Um, sounds good, let's ship it Friday.", "Slack", Tone::Casual, None, 172),
            sample(3, 38, "Hi Priya, thanks for the quick turnaround. The revised contract looks great; I'll sign it this afternoon.", "hi priya thanks for the quick turnaround uh the revised contract looks great I'll sign it this afternoon", "Mail", Tone::Formal, Some(310), 468),
            sample(2, 190, "fix the race condition in the audio thread before merging", "fix the race condition in the audio thread before merging", "Cursor", Tone::Verbatim, None, 141),
            sample(1, 1600, "Remember to book flights for the offsite and confirm the venue.", "Remember to book flights for the offsite and, uh, confirm the venue.", "Notes", Tone::Clean, None, 160),
        ];
        self.set_phase(Phase::Recording { command: false, hands_free: false }, cx);
        cx.spawn(async move |this, cx| {
            let mut t = 0f32;
            loop {
                cx.background_executor().timer(Duration::from_millis(33)).await;
                t += 0.033;
                let level = ((t * 7.0).sin() * 0.5 + 0.5) * ((t * 1.7).sin() * 0.4 + 0.6);
                const SCRIPT: &str = "Hey Priya, quick update on the launch. The new build is ready for testing and the latency is now under two hundred milliseconds, so let's ship it on Friday.";
                let words = ((t / 0.28) as usize) % (SCRIPT.split(' ').count() + 8);
                let live: String = SCRIPT.split(' ').take(words).collect::<Vec<_>>().join(" ");
                if this
                    .update(cx, |this, cx| {
                        this.levels.pop_front();
                        this.levels.push_back(level);
                        this.live_text = live.into();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    // ── Derived stats ───────────────────────────────────────────────────

    pub fn total_words(&self) -> usize {
        self.history.iter().map(|e| e.words()).sum()
    }

    /// Minutes saved vs. typing at 40 wpm.
    pub fn minutes_saved(&self) -> f64 {
        let typing_min = self.total_words() as f64 / 40.0;
        let speaking_min = self.history.iter().map(|e| e.audio_ms).sum::<u64>() as f64 / 60_000.0;
        (typing_min - speaking_min).max(0.0)
    }

    /// Median release-to-paste latency over recent dictations.
    pub fn median_latency_ms(&self) -> Option<u64> {
        let mut v: Vec<u64> = self.history.iter().take(30).map(|e| e.total_ms).collect();
        if v.is_empty() {
            return None;
        }
        v.sort_unstable();
        Some(v[v.len() / 2])
    }

    /// How many times faster than real time recent transcriptions ran.
    /// How much faster than real time `model` transcribed recent dictations.
    pub fn realtime_factor(&self, model: &str) -> Option<f64> {
        let (audio, asr) = self
            .history
            .iter()
            .filter(|e| e.model == model)
            .take(30)
            .fold((0u64, 0u64), |(a, b), e| (a + e.audio_ms, b + e.asr_ms));
        (asr > 0).then(|| audio as f64 / asr as f64)
    }
}

/// Downloads and unpacks a model's files, reporting progress (0..=1).
pub fn install_model(spec: &ModelSpec, progress: impl Fn(f32)) -> anyhow::Result<()> {
    match (spec.engine, spec.source) {
        (Engine::CoremlParakeet(cfg), _) => engine::parakeet_coreml::install(&spec.path(), cfg, progress),
        (Engine::CoremlBark(cfg), _) => engine::bark::install(&spec.path(), cfg, progress),
        (_, Source::Tarball(url)) => download_tarball(url, spec, progress),
        (_, Source::HuggingFace) => Err(anyhow!("{} has no download source", spec.id)),
    }
}

/// Downloads a `.tar.gz` model and unpacks it into the models folder.
fn download_tarball(url: &str, spec: &ModelSpec, progress: impl Fn(f32)) -> anyhow::Result<()> {
    let dir = store::models_dir();
    std::fs::create_dir_all(&dir)?;
    let archive = dir.join(format!("{}.tar.gz.part", spec.dir));
    let mut response = ureq::get(url).call().map_err(|e| anyhow!("{e}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(&archive)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if total > 0 {
            progress(done as f32 / total as f32);
        }
    }
    drop(file);
    let status = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&dir)
        .status()
        .context("could not run tar")?;
    let _ = std::fs::remove_file(&archive);
    if !status.success() || !spec.is_installed() {
        return Err(anyhow!("archive did not contain the model"));
    }
    Ok(())
}

/// First `max` characters of `text` on one line, with an ellipsis if cut.
fn preview(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    format!("{}…", line.chars().take(max - 1).collect::<String>().trim_end())
}
