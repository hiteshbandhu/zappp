//! The zappp app icon as a gpui element.

use std::sync::{Arc, OnceLock};

use gpui::{Image, ImageFormat, IntoElement, Styled, img, px};

const ICON_PNG: &[u8] = include_bytes!("../../brand/AppIcon.iconset/icon_256x256@2x.png");

fn icon_image() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, ICON_PNG.to_vec())))
        .clone()
}

/// The app icon at `size` points. The artwork already carries the macOS
/// icon margin, so it lines up with icons drawn by the system.
pub fn app_icon(size: f32) -> impl IntoElement {
    img(icon_image()).size(px(size)).flex_none()
}
