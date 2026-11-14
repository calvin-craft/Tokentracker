use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, path::PathBuf};
use tauri::{AppHandle, Manager};

pub const PROVIDERS: [&str; 5] = ["codex", "claude", "gemini", "antigravity", "grok"];
/// The providers a fresh install starts with; the others are added by hand.
const DEFAULT_PROVIDERS: [&str; 4] = ["codex", "claude", "gemini", "antigravity"];
/// Bumped when stored settings need a one-time update on load.
const VERSION: u32 = 3;
const MAX_ACCOUNTS: usize = 12;
const MAX_LABEL_CHARS: usize = 16;
/// Usage moves slowly, and every refresh is a request to the provider made
/// with the user's login: ask no more often than this.
pub const MIN_REFRESH_MINUTES: u32 = 3;
const DEFAULT_REFRESH_MINUTES: u32 = 5;

/// One tracked account.
///
/// The built-in account of a provider has `id == provider`. Built-in Codex
/// reads the Codex app's login on this PC; every other account is signed in to
/// in a window of ours. Further accounts of a provider get ids like
/// `claude-k3x9` and their own cookie store, so several logins can coexist.
/// Antigravity reads the Antigravity app's login on this PC, so it has only
/// the built-in account.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "yes")]
    pub shown: bool,
    /// Day of the month the plan is billed on (1–31), set by the user.
    #[serde(default)]
    pub billing_day: Option<u8>,
}

fn yes() -> bool {
    true
}

impl Account {
    fn builtin(provider: &str, shown: bool) -> Self {
        Self {
            id: provider.into(),
            provider: provider.into(),
            label: String::new(),
            shown,
            billing_day: None,
        }
    }

    pub fn is_builtin(&self) -> bool {
        self.id == self.provider
    }

    /// Built-in Codex and Antigravity are read from the apps on this PC, not
    /// from a webview.
    pub fn is_local(&self) -> bool {
        self.is_builtin() && matches!(self.provider.as_str(), "codex" | "antigravity")
    }

    pub fn provider_name(&self) -> &'static str {
        match self.provider.as_str() {
            "codex" => "Codex",
            "claude" => "Claude",
            "gemini" => "Gemini",
            "antigravity" => "Antigravity",
            "grok" => "Grok",
            _ => "?",
        }
    }

    /// Name for messages: "Claude" or "Claude (Work)".
    pub fn display_name(&self) -> String {
        if self.label.is_empty() {
            self.provider_name().to_string()
        } else {
            format!("{} ({})", self.provider_name(), self.label)
        }
    }

    fn is_valid(&self) -> bool {
        if !PROVIDERS.contains(&self.provider.as_str()) {
            return false;
        }
        if self.is_builtin() {
            return true;
        }
        if self.provider == "antigravity" {
            return false; // one login per PC: built-in only
        }
        // "<provider>-<suffix>"; the id becomes a window label and a folder name.
        match self.id.strip_prefix(&format!("{}-", self.provider)) {
            Some(suffix) => {
                (1..=16).contains(&suffix.len())
                    && suffix
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            }
            None => false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub refresh_minutes: u32,
    /// Widget background opacity, 0.2–1.0.
    pub opacity: f64,
    /// "bottom" (pinned to the desktop, under other windows), "top" (always
    /// on top) or "wallpaper" (part of the desktop, behind its icons).
    pub layer: String,
    /// Show one line per account instead of a full card.
    pub compact: bool,
    pub notify: bool,
    pub autostart: bool,
    /// "ko" or "en". Follows the system's language until the user picks one.
    pub language: String,
    /// While Antigravity is closed, start its language server for a moment
    /// to read the quota. Off by default: that runs Antigravity's own
    /// program, signed in as the user, from outside Antigravity.
    pub antigravity_launch: bool,
    pub accounts: Vec<Account>,
    /// Format of the stored file; absent (0) in files from before versions.
    #[serde(default)]
    pub version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            refresh_minutes: DEFAULT_REFRESH_MINUTES,
            opacity: 0.85,
            layer: "bottom".into(),
            compact: false,
            notify: true,
            autostart: false,
            language: crate::i18n::system_language().into(),
            antigravity_launch: false,
            accounts: DEFAULT_PROVIDERS
                .iter()
                .map(|p| Account::builtin(p, true))
                .collect(),
            version: VERSION,
        }
    }
}

impl Settings {
    /// Clamp values coming from disk or the UI into their valid ranges.
    pub fn sanitized(mut self) -> Self {
        self.refresh_minutes = self.refresh_minutes.clamp(MIN_REFRESH_MINUTES, 120);
        self.opacity = if self.opacity.is_finite() {
            self.opacity.clamp(0.2, 1.0)
        } else {
            0.85
        };
        // Wallpaper mode lives inside the Windows desktop; there is no such
        // place elsewhere.
        let wallpaper_ok = cfg!(windows) && self.layer == "wallpaper";
        if !(self.layer == "top" || wallpaper_ok) {
            self.layer = "bottom".into();
        }
        if self.language != "en" {
            self.language = "ko".into();
        }
        // Whatever comes from the UI is already in the current format.
        self.version = VERSION;

        let mut seen = HashSet::new();
        self.accounts
            .retain(|a| a.is_valid() && seen.insert(a.id.clone()));
        for account in &mut self.accounts {
            account.label = account.label.trim().chars().take(MAX_LABEL_CHARS).collect();
            account.billing_day = account.billing_day.filter(|d| (1..=31).contains(d));
        }
        self.accounts.truncate(MAX_ACCOUNTS);
        self
    }

    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

/// Returns the stored settings, or `None` on first run.
pub fn load(app: &AppHandle) -> Option<Settings> {
    let text = fs::read_to_string(path(app)?).ok()?;
    let raw: Value = serde_json::from_str(&text).ok()?;
    let mut settings: Settings = serde_json::from_value(raw.clone()).ok()?;

    // Files from before multi-account support have per-provider show flags.
    if raw.get("accounts").is_none() {
        let shown = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(true);
        settings.accounts = vec![
            Account::builtin("codex", shown("showCodex")),
            Account::builtin("claude", shown("showClaude")),
            Account::builtin("gemini", shown("showGemini")),
        ];
    }
    // Antigravity used to be shown inside the built-in Gemini account. It is
    // an account of its own now, placed right after Gemini.
    if settings.version < 2 {
        if let Some(i) = settings.accounts.iter().position(|a| a.id == "gemini") {
            let shown = settings.accounts[i].shown;
            settings.accounts.insert(i + 1, Account::builtin("antigravity", shown));
        }
    }
    // Version 2 and before refreshed every 2 minutes by default and allowed
    // every minute; move those to the new default.
    if settings.version < 3 && settings.refresh_minutes < MIN_REFRESH_MINUTES {
        settings.refresh_minutes = DEFAULT_REFRESH_MINUTES;
    }
    let migrated = settings.version < VERSION;
    settings.version = VERSION;
    let settings = settings.sanitized();
    if migrated {
        save(app, &settings);
    }
    Some(settings)
}

pub fn save(app: &AppHandle, settings: &Settings) {
    let Some(path) = path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(settings) {
        let _ = fs::write(path, text);
    }
}

/// Folder holding the cookie stores of user-added accounts.
pub fn profiles_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_local_data_dir().ok().map(|d| d.join("profiles"))
}

/// Cookie store of an account. Built-in accounts share the app's default one.
/// On macOS, whose webview ignores folders, see `profile_store_id` instead.
pub fn profile_dir(app: &AppHandle, account: &Account) -> Option<PathBuf> {
    if account.is_builtin() || cfg!(target_os = "macos") {
        None
    } else {
        profiles_dir(app).map(|d| d.join(&account.id))
    }
}

/// macOS names cookie stores by a UUID instead of a folder, and only from
/// macOS 14 on. Older versions put every account into the default store, so
/// added accounts share the built-in one's login there.
#[cfg(target_os = "macos")]
pub fn profile_store_id(account: &Account) -> Option<[u8; 16]> {
    if account.is_builtin() || !separate_stores() {
        return None;
    }
    // FNV-1a from two starting points: stable across builds, unlike std's
    // hasher, so an account finds its store again after an update.
    let hash = |seed: u64| {
        account
            .id
            .bytes()
            .fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
    };
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&hash(0xcbf2_9ce4_8422_2325).to_be_bytes());
    id[8..].copy_from_slice(&hash(0x6c62_272e_07bb_0142).to_be_bytes());
    // Version and variant bits of a well-formed (version 4) UUID.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    Some(id)
}

/// True on macOS 14 and later, which can keep cookie stores apart.
#[cfg(target_os = "macos")]
fn separate_stores() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|v| v.trim().split('.').next()?.parse::<u32>().ok())
            .is_some_and(|major| major >= 14)
    })
}

/// Delete cookie stores that no longer belong to any account.
pub fn remove_orphan_profiles(app: &AppHandle, settings: &Settings) {
    let Some(dir) = profiles_dir(app) else { return };
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if settings.account(&name).is_none() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
