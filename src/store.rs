//! Persistent settings, dictionary and history, stored as JSON under
//! `~/Library/Application Support/zappp`.

use std::{collections::BTreeMap, fs, path::PathBuf};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zappp");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn models_dir() -> PathBuf {
    data_dir().join("models")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hotkey {
    Fn,
    RightOption,
    RightCommand,
    RightControl,
}

impl Hotkey {
    pub const ALL: [Hotkey; 4] = [
        Hotkey::Fn,
        Hotkey::RightOption,
        Hotkey::RightCommand,
        Hotkey::RightControl,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Hotkey::Fn => "fn",
            Hotkey::RightOption => "Right ⌥",
            Hotkey::RightCommand => "Right ⌘",
            Hotkey::RightControl => "Right ⌃",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Hotkey::Fn => "fn",
            Hotkey::RightOption => "⌥",
            Hotkey::RightCommand => "⌘",
            Hotkey::RightControl => "⌃",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CleanupEngine {
    /// Rule-based clean-up only (fillers, spacing, dictionary). Instant and private.
    Local,
    /// Rule-based clean-up, then Claude for punctuation, formatting and tone.
    Claude,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AppCategory {
    Messaging,
    Email,
    Code,
    Documents,
    Everything,
}

impl AppCategory {
    pub const ALL: [AppCategory; 5] = [
        AppCategory::Messaging,
        AppCategory::Email,
        AppCategory::Code,
        AppCategory::Documents,
        AppCategory::Everything,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AppCategory::Messaging => "Messaging",
            AppCategory::Email => "Email",
            AppCategory::Code => "Code & terminals",
            AppCategory::Documents => "Notes & docs",
            AppCategory::Everything => "Everything else",
        }
    }

    pub fn examples(self) -> &'static str {
        match self {
            AppCategory::Messaging => "Slack, Messages, Discord, WhatsApp, Telegram",
            AppCategory::Email => "Mail, Outlook, Superhuman, Spark",
            AppCategory::Code => "Cursor, VS Code, Zed, Xcode, Terminal, Ghostty",
            AppCategory::Documents => "Notes, Notion, Obsidian, Pages, Word",
            AppCategory::Everything => "Any app not listed above",
        }
    }

    pub fn for_bundle(bundle_id: &str) -> AppCategory {
        let id = bundle_id.to_ascii_lowercase();
        let any = |list: &[&str]| list.iter().any(|p| id.starts_with(p));
        if any(&[
            "com.tinyspeck.slackmacgap",
            "com.apple.mobilesms",
            "com.hnc.discord",
            "net.whatsapp",
            "ru.keepcoder.telegram",
            "com.microsoft.teams",
            "com.facebook.archon",
        ]) {
            AppCategory::Messaging
        } else if any(&[
            "com.apple.mail",
            "com.microsoft.outlook",
            "com.superhuman",
            "com.readdle.smartemail",
            "com.readdle.spark",
        ]) {
            AppCategory::Email
        } else if any(&[
            "com.microsoft.vscode",
            "com.todesktop.230313mzl4w4u92",
            "com.apple.dt.xcode",
            "dev.zed",
            "com.apple.terminal",
            "com.googlecode.iterm2",
            "com.mitchellh.ghostty",
            "dev.warp",
            "com.jetbrains",
            "com.exafunction.windsurf",
        ]) {
            AppCategory::Code
        } else if any(&[
            "com.apple.notes",
            "notion.id",
            "md.obsidian",
            "com.apple.iwork.pages",
            "com.microsoft.word",
            "com.craft",
        ]) {
            AppCategory::Documents
        } else {
            AppCategory::Everything
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tone {
    /// Exactly what you said, dictionary applied.
    Verbatim,
    /// Fillers removed, punctuation fixed.
    Clean,
    /// Relaxed, chat-style.
    Casual,
    /// Polished and professional.
    Formal,
}

impl Tone {
    pub const ALL: [Tone; 4] = [Tone::Verbatim, Tone::Clean, Tone::Casual, Tone::Formal];

    pub fn label(self) -> &'static str {
        match self {
            Tone::Verbatim => "Verbatim",
            Tone::Clean => "Clean",
            Tone::Casual => "Casual",
            Tone::Formal => "Formal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: Hotkey,
    pub command_hotkey: Hotkey,
    pub cleanup: CleanupEngine,
    pub anthropic_api_key: String,
    pub remove_fillers: bool,
    pub press_enter_command: bool,
    pub restore_clipboard: bool,
    /// Press Return after pasting, so releasing the key sends the message.
    pub auto_submit: bool,
    /// Input device name; `None` follows the system default.
    pub microphone: Option<String>,
    pub show_idle_pill: bool,
    /// Id of the speech model in `models::MODELS`.
    pub model: String,
    pub tones: BTreeMap<AppCategory, Tone>,
    pub vocabulary: Vec<String>,
    pub replacements: Vec<Replacement>,
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: Hotkey::Fn,
            command_hotkey: Hotkey::RightOption,
            cleanup: CleanupEngine::Local,
            anthropic_api_key: String::new(),
            remove_fillers: true,
            press_enter_command: true,
            restore_clipboard: true,
            auto_submit: false,
            microphone: None,
            show_idle_pill: false,
            model: crate::models::DEFAULT_MODEL.to_string(),
            tones: BTreeMap::from([
                (AppCategory::Messaging, Tone::Casual),
                (AppCategory::Email, Tone::Formal),
                (AppCategory::Code, Tone::Verbatim),
                (AppCategory::Documents, Tone::Clean),
                (AppCategory::Everything, Tone::Clean),
            ]),
            vocabulary: vec!["zappp".into(), "GPUI".into()],
            replacements: vec![Replacement {
                from: "zap app".into(),
                to: "zappp".into(),
            }],
            onboarded: false,
        }
    }
}

impl Settings {
    pub fn tone_for(&self, category: AppCategory) -> Tone {
        self.tones.get(&category).copied().unwrap_or(Tone::Clean)
    }

    pub fn api_key(&self) -> Option<String> {
        let key = self.anthropic_api_key.trim();
        if !key.is_empty() {
            return Some(key.to_string());
        }
        std::env::var("ANTHROPIC_API_KEY").ok().filter(|k| !k.is_empty())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub at: DateTime<Local>,
    pub raw: String,
    pub text: String,
    pub app_name: String,
    pub bundle_id: String,
    pub tone: Tone,
    pub command: bool,
    pub audio_ms: u64,
    pub asr_ms: u64,
    /// Id of the speech model that transcribed it (empty for older entries).
    #[serde(default)]
    pub model: String,
    pub llm_ms: Option<u64>,
    /// Key release → text pasted.
    pub total_ms: u64,
}

impl HistoryEntry {
    pub fn words(&self) -> usize {
        self.text.split_whitespace().count()
    }
}

fn load_json<T: DeserializeOwned + Default>(name: &str) -> T {
    fs::read(data_dir().join(name))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_json<T: Serialize>(name: &str, value: &T) {
    // The demo (ZAPPP_DEMO) shows sample data; never write it over the
    // user's real settings and history.
    if std::env::var_os("ZAPPP_DEMO").is_some() {
        return;
    }
    let path = data_dir().join(name);
    let tmp = path.with_extension("json.tmp");
    match serde_json::to_vec_pretty(value) {
        Ok(bytes) => {
            if fs::write(&tmp, bytes).and_then(|_| fs::rename(&tmp, &path)).is_err() {
                log::warn!("failed to save {}", path.display());
            }
        }
        Err(err) => log::warn!("failed to serialize {name}: {err}"),
    }
}

pub fn load_settings() -> Settings {
    load_json("settings.json")
}

pub fn save_settings(settings: &Settings) {
    save_json("settings.json", settings)
}

pub fn load_history() -> Vec<HistoryEntry> {
    load_json("history.json")
}

pub fn save_history(history: &[HistoryEntry]) {
    save_json("history.json", &history)
}
