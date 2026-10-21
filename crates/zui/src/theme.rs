use gpui::{App, BoxShadow, Global, Hsla, Pixels, WindowAppearance, point, px, rgb, rgba};

/// Design tokens. Modeled on macOS system colors so zappp sits next to
/// System Settings and Notes without looking out of place: system font,
/// grouped rows on a quiet fill, hairlines instead of drop shadows, and the
/// system accent as the only color.
#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,

    // Surfaces
    /// Window content background.
    pub canvas: Hsla,
    /// Translucent sidebar over the window's vibrancy.
    pub sidebar: Hsla,
    /// Fill for grouped rows (the "inset list" look).
    pub group: Hsla,
    /// Push-button / text-field fill.
    pub control: Hsla,
    pub control_hover: Hsla,
    /// Selected sidebar row, segmented track, chips.
    pub fill_selected: Hsla,
    pub fill_subtle: Hsla,

    // Lines
    pub separator: Hsla,
    pub border: Hsla,

    // Text
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,

    // Intent
    pub accent: Hsla,
    pub accent_hover: Hsla,
    pub on_accent: Hsla,
    pub focus_ring: Hsla,
    pub green: Hsla,
    pub orange: Hsla,
    pub red: Hsla,

    // Shape
    pub radius_sm: Pixels,
    pub radius: Pixels,
    pub radius_lg: Pixels,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            dark: false,
            canvas: rgb(0xffffff).into(),
            sidebar: rgba(0xf5f5f7d9).into(),
            group: rgba(0x0000000a).into(),
            control: rgb(0xffffff).into(),
            control_hover: rgb(0xf7f7f7).into(),
            fill_selected: rgba(0x0000001a).into(),
            fill_subtle: rgba(0x0000000d).into(),

            separator: rgba(0x00000017).into(),
            border: rgba(0x00000021).into(),

            text: rgba(0x000000d9).into(),
            text_secondary: rgba(0x0000008c).into(),
            text_tertiary: rgba(0x00000059).into(),

            accent: rgb(0x007aff).into(),
            accent_hover: rgb(0x0069d9).into(),
            on_accent: rgb(0xffffff).into(),
            focus_ring: rgba(0x007aff59).into(),
            green: rgb(0x28a745).into(),
            orange: rgb(0xc66a00).into(),
            red: rgb(0xd70015).into(),

            radius_sm: px(5.),
            radius: px(7.),
            radius_lg: px(10.),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            canvas: rgb(0x1e1e1e).into(),
            sidebar: rgba(0x2a2a2acc).into(),
            group: rgba(0xffffff0d).into(),
            control: rgba(0xffffff1f).into(),
            control_hover: rgba(0xffffff29).into(),
            fill_selected: rgba(0xffffff1a).into(),
            fill_subtle: rgba(0xffffff12).into(),

            separator: rgba(0xffffff14).into(),
            border: rgba(0xffffff1f).into(),

            text: rgba(0xffffffd9).into(),
            text_secondary: rgba(0xffffff8c).into(),
            text_tertiary: rgba(0xffffff52).into(),

            accent: rgb(0x0a84ff).into(),
            accent_hover: rgb(0x3395ff).into(),
            on_accent: rgb(0xffffff).into(),
            focus_ring: rgba(0x0a84ff66).into(),
            green: rgb(0x32d74b).into(),
            orange: rgb(0xff9f0a).into(),
            red: rgb(0xff453a).into(),

            radius_sm: px(5.),
            radius: px(7.),
            radius_lg: px(10.),
        }
    }

    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            _ => Self::light(),
        }
    }

    /// The one-pixel lift native push buttons and segmented thumbs have.
    /// Nothing else in the app casts a shadow.
    pub fn shadow_control(&self) -> Vec<BoxShadow> {
        if self.dark {
            vec![]
        } else {
            vec![
                BoxShadow {
                    color: rgba(0x0000001f).into(),
                    offset: point(px(0.), px(0.5)),
                    blur_radius: px(1.),
                    spread_radius: px(0.),
                },
            ]
        }
    }
}

impl Global for Theme {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}
