use crate::settings::Account;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub const STATE_OK: &str = "ok";
pub const STATE_NEEDS_LOGIN: &str = "needs_login";
pub const STATE_ERROR: &str = "error";
pub const STATE_LOADING: &str = "loading";

/// One usage window of a plan (e.g. the 5-hour or weekly limit).
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub key: String,
    pub label: String,
    pub used_percent: f64,
    /// Epoch seconds.
    pub resets_at: Option<i64>,
    /// Length of the window in seconds, when the provider tells.
    pub period_seconds: Option<i64>,
    /// Heading for limits that come from another app than the account's own
    /// (Antigravity's, inside the Gemini account).
    pub group: Option<String>,
}

/// What we know about one account right now.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    /// The account id (see `settings::Account`).
    pub id: String,
    /// "codex", "claude", "gemini" or "grok".
    pub provider: String,
    /// Display name of the provider.
    pub name: String,
    /// The user's nickname for this account, possibly empty.
    pub label: String,
    pub state: String,
    pub plan: Option<String>,
    pub windows: Vec<LimitWindow>,
    pub message: Option<String>,
    /// Remark about the grouped limits, e.g. why there are none.
    pub note: Option<String>,
    /// Epoch seconds of the last successful fetch.
    pub updated_at: Option<i64>,
}

impl ProviderStatus {
    pub fn new(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            provider: account.provider.clone(),
            name: account.provider_name().into(),
            label: account.label.clone(),
            state: STATE_LOADING.into(),
            plan: None,
            windows: Vec::new(),
            message: None,
            note: None,
            updated_at: None,
        }
    }

    pub fn needs_login(mut self, message: &str) -> Self {
        self.state = STATE_NEEDS_LOGIN.into();
        self.message = Some(message.into());
        self
    }

    pub fn error(mut self, message: impl Into<String>) -> Self {
        self.state = STATE_ERROR.into();
        self.message = Some(message.into());
        self
    }

    pub fn ok(mut self, plan: Option<String>, windows: Vec<LimitWindow>) -> Self {
        self.state = STATE_OK.into();
        self.plan = plan;
        self.windows = windows;
        self.updated_at = Some(now_secs());
        self
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Snapshot {
    pub providers: Vec<ProviderStatus>,
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
