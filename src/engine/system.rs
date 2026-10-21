//! Thin wrappers over macOS APIs: synthetic keystrokes, the frontmost app,
//! selected text through Accessibility, and permission checks.

use std::{ffi::c_void, ptr};

use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    dictionary::CFDictionary,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use objc2::msg_send;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;

type AXUIElementRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

const KEY_V: CGKeyCode = 9;
const KEY_RETURN: CGKeyCode = 36;

fn post_key(code: CGKeyCode, flags: CGEventFlags) {
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::CombinedSessionState) else {
        return;
    };
    for down in [true, false] {
        if let Ok(event) = CGEvent::new_keyboard_event(source.clone(), code, down) {
            event.set_flags(flags);
            event.post(CGEventTapLocation::HID);
        }
    }
}

pub fn send_paste() {
    post_key(KEY_V, CGEventFlags::CGEventFlagCommand);
}

pub fn send_return() {
    post_key(KEY_RETURN, CGEventFlags::CGEventFlagNull);
}

#[derive(Clone, Debug, Default)]
pub struct FrontApp {
    pub name: String,
    pub bundle_id: String,
}

pub fn frontmost_app() -> FrontApp {
    let workspace = NSWorkspace::sharedWorkspace();
    let Some(app) = workspace.frontmostApplication() else {
        return FrontApp::default();
    };
    FrontApp {
        name: app.localizedName().map(|s| s.to_string()).unwrap_or_default(),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()).unwrap_or_default(),
    }
}

/// The selected text in the focused field of any app, via Accessibility.
pub fn selected_text() -> Option<String> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let focused = copy_attr(system, "AXFocusedUIElement");
        CFRelease(system);
        let focused = focused?;
        let text = copy_attr(focused, "AXSelectedText");
        CFRelease(focused);
        let text = CFType::wrap_under_create_rule(text?);
        let text = text.downcast::<CFString>()?.to_string();
        (!text.trim().is_empty()).then_some(text)
    }
}

unsafe fn copy_attr(element: AXUIElementRef, name: &'static str) -> Option<CFTypeRef> {
    let attr = CFString::from_static_string(name);
    let mut value: CFTypeRef = ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(element, attr.as_concrete_TypeRef(), &mut value) };
    (err == 0 && !value.is_null()).then_some(value)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    pub microphone: MicAccess,
    pub accessibility: bool,
    pub input_monitoring: bool,
}

impl Permissions {
    pub fn all_granted(&self) -> bool {
        self.microphone == MicAccess::Granted && self.accessibility && self.input_monitoring
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MicAccess {
    /// macOS hasn't asked yet.
    #[default]
    NotAsked,
    Denied,
    Granted,
}

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

pub fn microphone_access() -> MicAccess {
    // AVAuthorizationStatus: 0 not determined, 1 restricted, 2 denied, 3 authorized.
    let status: isize = unsafe {
        msg_send![objc2::class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio]
    };
    match status {
        3 => MicAccess::Granted,
        0 => MicAccess::NotAsked,
        _ => MicAccess::Denied,
    }
}

/// Shows the system microphone prompt (only the first time).
pub fn request_microphone() {
    let handler = block2::RcBlock::new(|granted: objc2::runtime::Bool| {
        log::info!("microphone access granted: {}", granted.as_bool());
    });
    unsafe {
        let _: () = msg_send![
            objc2::class!(AVCaptureDevice),
            requestAccessForMediaType: AVMediaTypeAudio,
            completionHandler: &*handler
        ];
    }
}

pub fn permissions() -> Permissions {
    unsafe {
        Permissions {
            microphone: microphone_access(),
            accessibility: AXIsProcessTrusted(),
            input_monitoring: CGPreflightListenEventAccess(),
        }
    }
}

/// Size of the camera notch on the main screen, in points. On Macs without a
/// notch the width is 0 and the height is the menu bar's, so the pill still
/// hangs from the top edge like a notch.
#[derive(Clone, Copy, Debug)]
pub struct Notch {
    pub width: f32,
    pub height: f32,
}

pub fn notch() -> Notch {
    use objc2_app_kit::NSScreen;
    let fallback = Notch { width: 0., height: 24. };
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return fallback;
    };
    // The first screen is the one with the menu bar.
    let screens = NSScreen::screens(mtm);
    let Some(screen) = screens.firstObject() else {
        return fallback;
    };
    let frame = screen.frame();
    let top_inset = screen.safeAreaInsets().top;
    if top_inset > 0. {
        let left = screen.auxiliaryTopLeftArea();
        let right = screen.auxiliaryTopRightArea();
        Notch {
            width: (frame.size.width - left.size.width - right.size.width).max(0.) as f32,
            height: top_inset as f32,
        }
    } else {
        let visible = screen.visibleFrame();
        let menu_bar = frame.origin.y + frame.size.height - (visible.origin.y + visible.size.height);
        Notch {
            width: 0.,
            height: if menu_bar > 0. { menu_bar as f32 } else { 24. },
        }
    }
}

/// Shows zappp in the Dock and ⌘-Tab (while its window is open) or keeps it
/// menu-bar-only. Call outside gpui updates: AppKit re-enters the app.
pub fn set_dock_visible(visible: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if visible {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    app.setActivationPolicy(policy);
}

/// Whether macOS will deliver keyboard events to an event tap we create.
pub fn input_monitoring_allowed() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

/// Shows the system Accessibility prompt (once per app identity).
pub fn request_accessibility() {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    unsafe {
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void);
    }
}

pub fn request_input_monitoring() {
    unsafe {
        CGRequestListenEventAccess();
    }
}

pub const SETTINGS_MICROPHONE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
pub const SETTINGS_ACCESSIBILITY: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
pub const SETTINGS_INPUT_MONITORING: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
