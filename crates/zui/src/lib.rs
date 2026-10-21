//! zui — the component library behind zappp.
//!
//! Primitives on top of gpui that look at home on macOS: system colors and
//! font, grouped lists, native-sized controls and Lucide icons.

mod badge;
mod button;
mod icon;
mod kbd;
mod misc;
mod nav;
mod segmented;
mod switch;
mod text_input;
mod theme;
mod waveform;

use std::borrow::Cow;

use gpui::{App, AssetSource, SharedString};
use rust_embed::RustEmbed;

pub use badge::{Badge, Tone};
pub use button::{Button, ButtonSize, ButtonVariant};
pub use icon::{Icon, IconName};
pub use kbd::Kbd;
pub use misc::{Divider, group, group_header, h_stack, v_stack};
pub use nav::NavItem;
pub use segmented::Segmented;
pub use switch::Switch;
pub use text_input::{TextInput, TextInputEvent};
pub use theme::{ActiveTheme, Theme};
pub use waveform::Waveform;

#[derive(RustEmbed)]
#[folder = "assets"]
#[include = "icons/*.svg"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|file| file.data))
    }

    fn list(&self, path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Self::iter()
            .filter(|p| p.starts_with(path))
            .map(SharedString::from)
            .collect())
    }
}

/// Installs the theme (following the system appearance) and binds keys used
/// by zui controls.
/// Call once, before opening any window.
pub fn init(cx: &mut App) {
    cx.set_global(Theme::for_appearance(cx.window_appearance()));

    text_input::bind_keys(cx);
}
