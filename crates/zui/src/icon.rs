use gpui::{App, Hsla, IntoElement, Pixels, RenderOnce, Styled, Window, px, svg};

use crate::ActiveTheme;

/// Lucide icons bundled with zui (ISC licensed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    ArrowUpRight,
    AudioLines,
    BookA,
    Check,
    ChevronRight,
    CircleAlert,
    CircleCheck,
    Clock,
    Cloud,
    Code,
    Command,
    Copy,
    CornerDownLeft,
    Cpu,
    Download,
    Ellipsis,
    FileText,
    Globe,
    History,
    House,
    Keyboard,
    Languages,
    Lock,
    Mail,
    MessageSquare,
    Mic,
    Pause,
    Plus,
    Search,
    Settings,
    ShieldCheck,
    Sliders,
    Square,
    Trash,
    Type,
    X,
}

impl IconName {
    pub fn path(self) -> &'static str {
        match self {
            Self::ArrowUpRight => "icons/arrow-up-right.svg",
            Self::AudioLines => "icons/audio-lines.svg",
            Self::BookA => "icons/book-a.svg",
            Self::Check => "icons/check.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::CircleAlert => "icons/circle-alert.svg",
            Self::CircleCheck => "icons/circle-check.svg",
            Self::Clock => "icons/clock.svg",
            Self::Cloud => "icons/cloud.svg",
            Self::Code => "icons/code.svg",
            Self::Command => "icons/command.svg",
            Self::Copy => "icons/copy.svg",
            Self::CornerDownLeft => "icons/corner-down-left.svg",
            Self::Cpu => "icons/cpu.svg",
            Self::Download => "icons/download.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::FileText => "icons/file-text.svg",
            Self::Globe => "icons/globe.svg",
            Self::History => "icons/history.svg",
            Self::House => "icons/house.svg",
            Self::Keyboard => "icons/keyboard.svg",
            Self::Languages => "icons/languages.svg",
            Self::Lock => "icons/lock.svg",
            Self::Mail => "icons/mail.svg",
            Self::MessageSquare => "icons/message-square.svg",
            Self::Mic => "icons/mic.svg",
            Self::Pause => "icons/pause.svg",
            Self::Plus => "icons/plus.svg",
            Self::Search => "icons/search.svg",
            Self::Settings => "icons/settings-2.svg",
            Self::ShieldCheck => "icons/shield-check.svg",
            Self::Sliders => "icons/sliders-horizontal.svg",
            Self::Square => "icons/square.svg",
            Self::Trash => "icons/trash-2.svg",
            Self::Type => "icons/type.svg",
            Self::X => "icons/x.svg",
        }
    }
}

#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: Pixels,
    color: Option<Hsla>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: px(16.),
            color: None,
        }
    }

    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = size.into();
        self
    }

    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        svg()
            .path(self.name.path())
            .size(self.size)
            .flex_none()
            .text_color(self.color.unwrap_or(cx.theme().text_secondary))
    }
}
