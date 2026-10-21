//! Menu bar item (NSStatusItem). gpui has no tray API, so this talks to
//! AppKit directly and forwards clicks over a channel.
//!
//! The app pushes a [`MenuState`] snapshot whenever something the menu shows
//! changes; the menu is rebuilt from it, so it's always current when opened.

use std::{cell::RefCell, sync::OnceLock};

use futures::channel::mpsc::UnboundedSender;
use objc2::{
    AnyThread as _, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
    runtime::{NSObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSSize, NSString};

#[derive(Clone, Debug)]
pub enum TrayEvent {
    Open,
    History,
    Settings,
    Welcome,
    Quit,
    /// Start or stop hands-free dictation into the frontmost app.
    ToggleDictation,
    CopyLast,
    PasteLast,
    ToggleAutoSubmit,
    CopyRecent(usize),
    /// `None` is the system default input.
    Microphone(Option<String>),
    Model(&'static str),
}

/// Everything the menu shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuState {
    pub status: String,
    pub can_dictate: bool,
    pub dictating: bool,
    /// Short previews of the most recent transcripts, newest first.
    pub recent: Vec<String>,
    pub microphones: Vec<String>,
    pub microphone: Option<String>,
    /// Installed models: (id, display name).
    pub models: Vec<(&'static str, String)>,
    pub model: &'static str,
    pub auto_submit: bool,
}

// Menu item tags. Lists use a base plus their index.
const TAG_OPEN: isize = 1;
const TAG_HISTORY: isize = 2;
const TAG_SETTINGS: isize = 3;
const TAG_WELCOME: isize = 4;
const TAG_QUIT: isize = 5;
const TAG_DICTATE: isize = 6;
const TAG_COPY_LAST: isize = 7;
const TAG_PASTE_LAST: isize = 8;
const TAG_AUTO_SUBMIT: isize = 9;
const TAG_RECENT: isize = 100;
const TAG_MIC_DEFAULT: isize = 200;
const TAG_MIC: isize = 201;
const TAG_MODEL: isize = 300;

static EVENTS: OnceLock<UnboundedSender<TrayEvent>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZapppTrayTarget"]
    struct TrayTarget;

    impl TrayTarget {
        #[unsafe(method(menuItem:))]
        fn menu_item(&self, sender: Option<&NSMenuItem>) {
            if let Some(event) = sender.and_then(|item| event_for(item.tag())) {
                send(event);
            }
        }
    }
);

impl TrayTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

pub fn send(event: TrayEvent) {
    if let Some(tx) = EVENTS.get() {
        let _ = tx.unbounded_send(event);
    }
}

struct Tray {
    item: Retained<NSStatusItem>,
    target: Retained<TrayTarget>,
    state: MenuState,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

fn event_for(tag: isize) -> Option<TrayEvent> {
    let state = TRAY.with(|t| t.borrow().as_ref().map(|t| t.state.clone()))?;
    Some(match tag {
        TAG_OPEN => TrayEvent::Open,
        TAG_HISTORY => TrayEvent::History,
        TAG_SETTINGS => TrayEvent::Settings,
        TAG_WELCOME => TrayEvent::Welcome,
        TAG_QUIT => TrayEvent::Quit,
        TAG_DICTATE => TrayEvent::ToggleDictation,
        TAG_COPY_LAST => TrayEvent::CopyLast,
        TAG_PASTE_LAST => TrayEvent::PasteLast,
        TAG_AUTO_SUBMIT => TrayEvent::ToggleAutoSubmit,
        TAG_MIC_DEFAULT => TrayEvent::Microphone(None),
        t if (TAG_RECENT..TAG_MIC_DEFAULT).contains(&t) => TrayEvent::CopyRecent((t - TAG_RECENT) as usize),
        t if (TAG_MIC..TAG_MODEL).contains(&t) => {
            TrayEvent::Microphone(Some(state.microphones.get((t - TAG_MIC) as usize)?.clone()))
        }
        t if t >= TAG_MODEL => TrayEvent::Model(state.models.get((t - TAG_MODEL) as usize)?.0),
        _ => return None,
    })
}

const GLYPH: &[u8] = include_bytes!("../../assets/menubar@2x.png");
const GLYPH_RECORDING: &[u8] = include_bytes!("../../assets/menubar-recording@2x.png");

/// The zappp mark as an 18 pt template image (macOS tints it for the menu bar).
fn glyph(recording: bool) -> Option<Retained<NSImage>> {
    let bytes = if recording { GLYPH_RECORDING } else { GLYPH };
    let data = NSData::with_bytes(bytes);
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    image.setSize(NSSize::new(18., 18.));
    image.setTemplate(true);
    Some(image)
}

/// Builds menus with one shared target.
struct Builder<'a> {
    mtm: MainThreadMarker,
    target: &'a TrayTarget,
}

impl Builder<'_> {
    fn item(&self, menu: &NSMenu, title: &str, tag: Option<isize>) -> Retained<NSMenuItem> {
        let action: Option<Sel> = tag.map(|_| sel!(menuItem:));
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm),
                &NSString::from_str(title),
                action,
                &NSString::from_str(""),
            )
        };
        if let Some(tag) = tag {
            item.setTag(tag);
            unsafe { item.setTarget(Some(self.target)) };
        }
        menu.addItem(&item);
        item
    }

    fn checked(&self, menu: &NSMenu, title: &str, tag: isize, on: bool) {
        let item = self.item(menu, title, Some(tag));
        item.setState(if on { 1 } else { 0 });
    }

    fn separator(&self, menu: &NSMenu) {
        menu.addItem(&NSMenuItem::separatorItem(self.mtm));
    }

    fn submenu(&self, menu: &NSMenu, title: &str, fill: impl FnOnce(&NSMenu)) {
        let sub = NSMenu::new(self.mtm);
        fill(&sub);
        let item = self.item(menu, title, None);
        item.setSubmenu(Some(&sub));
    }

    fn shortcut(item: &NSMenuItem, key: &str) {
        item.setKeyEquivalent(&NSString::from_str(key));
        item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    }

    fn build(&self, s: &MenuState) -> Retained<NSMenu> {
        let menu = NSMenu::new(self.mtm);
        self.item(&menu, &s.status, None);
        self.separator(&menu);

        let dictate = if s.dictating { "Stop Dictation" } else { "Start Dictation" };
        self.item(&menu, dictate, (s.can_dictate || s.dictating).then_some(TAG_DICTATE));
        let has_last = !s.recent.is_empty();
        self.item(&menu, "Paste Last Transcript", has_last.then_some(TAG_PASTE_LAST));
        self.item(&menu, "Copy Last Transcript", has_last.then_some(TAG_COPY_LAST));
        self.submenu(&menu, "Recent", |sub| {
            if s.recent.is_empty() {
                self.item(sub, "Nothing yet", None);
            }
            for (i, text) in s.recent.iter().enumerate() {
                self.item(sub, text, Some(TAG_RECENT + i as isize));
            }
            if !s.recent.is_empty() {
                self.separator(sub);
                self.item(sub, "Click one to copy it", None);
            }
        });
        self.separator(&menu);

        self.submenu(&menu, "Microphone", |sub| {
            let default = s.microphones.first().map(|n| format!("System Default ({n})"));
            self.checked(
                sub,
                default.as_deref().unwrap_or("System Default"),
                TAG_MIC_DEFAULT,
                s.microphone.is_none(),
            );
            self.separator(sub);
            for (i, name) in s.microphones.iter().enumerate() {
                self.checked(sub, name, TAG_MIC + i as isize, s.microphone.as_deref() == Some(name));
            }
        });
        self.submenu(&menu, "Speech Model", |sub| {
            for (i, (id, name)) in s.models.iter().enumerate() {
                self.checked(sub, name, TAG_MODEL + i as isize, *id == s.model);
            }
            self.separator(sub);
            self.item(sub, "Download more in Settings", None);
        });
        self.checked(&menu, "Press Return After Pasting", TAG_AUTO_SUBMIT, s.auto_submit);
        self.separator(&menu);

        self.item(&menu, "Open zappp", Some(TAG_OPEN));
        self.item(&menu, "History…", Some(TAG_HISTORY));
        let settings = self.item(&menu, "Settings…", Some(TAG_SETTINGS));
        Self::shortcut(&settings, ",");
        self.item(&menu, "Welcome Guide…", Some(TAG_WELCOME));
        self.separator(&menu);
        let quit = self.item(&menu, "Quit zappp", Some(TAG_QUIT));
        Self::shortcut(&quit, "q");
        menu
    }
}

/// Installs the menu bar item. Must run on the main thread.
pub fn install(tx: UnboundedSender<TrayEvent>) {
    let _ = EVENTS.set(tx);
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let target = TrayTarget::new(mtm);
    let state = MenuState {
        status: "Starting…".into(),
        ..Default::default()
    };
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    if let Some(button) = item.button(mtm) {
        button.setImage(glyph(false).as_deref());
    }
    let menu = Builder { mtm, target: &target }.build(&state);
    item.setMenu(Some(&menu));
    TRAY.with(|t| *t.borrow_mut() = Some(Tray { item, target, state }));
}

/// Rebuilds the menu if what it shows has changed.
pub fn set_state(state: MenuState) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    TRAY.with(|t| {
        let mut t = t.borrow_mut();
        let Some(tray) = t.as_mut() else {
            return;
        };
        if tray.state == state {
            return;
        }
        let menu = Builder { mtm, target: &tray.target }.build(&state);
        tray.item.setMenu(Some(&menu));
        tray.state = state;
    });
}

/// Swaps the menu bar glyph while recording.
pub fn set_recording(recording: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    TRAY.with(|t| {
        if let Some(tray) = t.borrow().as_ref()
            && let Some(button) = tray.item.button(mtm)
        {
            button.setImage(glyph(recording).as_deref());
        }
    });
}
