//! Global hold-to-talk keys via a listen-only `CGEventTap` (needs the Input
//! Monitoring permission). Runs its own CFRunLoop thread.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    thread,
    time::Duration,
};

use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use futures::channel::mpsc::UnboundedSender;

use crate::store::Hotkey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyEvent {
    DictateDown,
    DictateUp,
    CommandDown,
    CommandUp,
    Escape,
    /// Another key was pressed while the dictation key was held, so the user
    /// is typing a shortcut (⌘C, ⌘Tab…), not dictating.
    Chord,
}

/// Shared, live-updatable key assignment.
pub struct HotkeyConfig {
    dictate: AtomicU8,
    command: AtomicU8,
    pub tap_running: AtomicBool,
}

impl HotkeyConfig {
    pub fn new(dictate: Hotkey, command: Hotkey) -> Arc<Self> {
        Arc::new(Self {
            dictate: AtomicU8::new(encode(dictate)),
            command: AtomicU8::new(encode(command)),
            tap_running: AtomicBool::new(false),
        })
    }

    pub fn set(&self, dictate: Hotkey, command: Hotkey) {
        self.dictate.store(encode(dictate), Ordering::Relaxed);
        self.command.store(encode(command), Ordering::Relaxed);
    }

    fn dictate(&self) -> Hotkey {
        decode(self.dictate.load(Ordering::Relaxed))
    }

    fn command(&self) -> Hotkey {
        decode(self.command.load(Ordering::Relaxed))
    }
}

fn encode(k: Hotkey) -> u8 {
    Hotkey::ALL.iter().position(|h| *h == k).unwrap_or(0) as u8
}

fn decode(v: u8) -> Hotkey {
    Hotkey::ALL.get(v as usize).copied().unwrap_or(Hotkey::Fn)
}

/// macOS virtual keycode + device-specific modifier mask for each key.
fn key_info(k: Hotkey) -> (i64, u64) {
    match k {
        Hotkey::Fn => (63, CGEventFlags::CGEventFlagSecondaryFn.bits()),
        Hotkey::RightOption => (61, 0x40),   // NX_DEVICERALTKEYMASK
        Hotkey::RightCommand => (54, 0x10),  // NX_DEVICERCMDKEYMASK
        Hotkey::RightControl => (62, 0x2000), // NX_DEVICERCTLKEYMASK
    }
}

const KEY_ESCAPE: i64 = 53;

static WARNED: AtomicBool = AtomicBool::new(false);

pub fn spawn(config: Arc<HotkeyConfig>, tx: UnboundedSender<HotkeyEvent>) {
    thread::Builder::new()
        .name("zappp-hotkey".into())
        .spawn(move || {
            // Retry until the user grants Input Monitoring.
            loop {
                if run_tap(&config, &tx) {
                    break;
                }
                thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("spawn hotkey thread");
}

/// Returns false if the tap could not be created.
fn run_tap(config: &Arc<HotkeyConfig>, tx: &UnboundedSender<HotkeyEvent>) -> bool {
    // A tap created before Input Monitoring is granted exists but never sees
    // a key, and stays deaf after the grant. Wait for the permission so the
    // shortcut starts working the moment it's allowed — no restart needed.
    if !super::system::input_monitoring_allowed() {
        if !WARNED.swap(true, Ordering::Relaxed) {
            log::warn!("waiting for Input Monitoring permission before watching the keyboard");
        }
        return false;
    }
    let cfg = config.clone();
    let tx = tx.clone();
    let dictate_down = AtomicBool::new(false);
    let command_down = AtomicBool::new(false);

    let tap = CGEventTap::new(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
        move |_proxy, event_type, event| {
            let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
            match event_type {
                CGEventType::KeyDown if keycode == KEY_ESCAPE => {
                    let _ = tx.unbounded_send(HotkeyEvent::Escape);
                }
                CGEventType::KeyDown
                    if dictate_down.load(Ordering::Relaxed) || command_down.load(Ordering::Relaxed) =>
                {
                    let _ = tx.unbounded_send(HotkeyEvent::Chord);
                }
                CGEventType::FlagsChanged => {
                    let flags = event.get_flags().bits();
                    log::debug!("modifier key {keycode}, flags {flags:#x}");
                    for (key, state, down_ev, up_ev) in [
                        (cfg.dictate(), &dictate_down, HotkeyEvent::DictateDown, HotkeyEvent::DictateUp),
                        (cfg.command(), &command_down, HotkeyEvent::CommandDown, HotkeyEvent::CommandUp),
                    ] {
                        let (code, mask) = key_info(key);
                        if keycode != code {
                            continue;
                        }
                        let pressed = flags & mask != 0;
                        if pressed != state.swap(pressed, Ordering::Relaxed) {
                            let _ = tx.unbounded_send(if pressed { down_ev } else { up_ev });
                        }
                    }
                }
                _ => {}
            }
            None
        },
    );

    let Ok(tap) = tap else {
        if config.tap_running.swap(false, Ordering::Relaxed) || !WARNED.swap(true, Ordering::Relaxed) {
            log::warn!("cannot watch the keyboard yet: allow zappp in Privacy & Security → Input Monitoring");
        }
        return false;
    };
    let Ok(source) = tap.mach_port.create_runloop_source(0) else {
        return false;
    };
    unsafe {
        CFRunLoop::get_current().add_source(&source, kCFRunLoopCommonModes);
    }
    tap.enable();
    config.tap_running.store(true, Ordering::Relaxed);
    log::info!("hotkey tap running");
    CFRunLoop::run_current();
    true
}
