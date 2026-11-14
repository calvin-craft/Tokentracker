//! Codex / ChatGPT plan usage for the built-in account.
//!
//! Reads the login that the Codex app keeps in `~/.codex/auth.json` and asks
//! ChatGPT for the current rate-limit windows. The token is only ever sent to
//! chatgpt.com, and we never refresh it ourselves — Codex owns that file, and
//! rotating its refresh token from here would log Codex out.

use crate::{
    i18n::{self, t},
    model::{LimitWindow, ProviderStatus},
    settings::Account,
};
use serde_json::Value;
use std::{fs, path::PathBuf, sync::OnceLock, time::Duration};

const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("TokenTracker/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client")
    })
}

fn codex_home() -> Option<PathBuf> {
    match std::env::var("CODEX_HOME") {
        Ok(p) if !p.is_empty() => Some(PathBuf::from(p)),
        _ => dirs::home_dir().map(|h| h.join(".codex")),
    }
}

/// (access_token, account_id) of the ChatGPT login, if there is one.
fn read_auth() -> Option<(String, Option<String>)> {
    let text = fs::read_to_string(codex_home()?.join("auth.json")).ok()?;
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let tokens = json.get("tokens")?;
    let access = tokens.get("access_token")?.as_str()?.to_string();
    if access.is_empty() {
        return None;
    }
    let account = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    Some((access, account))
}

/// A rate-limit window as ChatGPT reports it, shared with the web accounts.
pub fn limit_window(
    key: &str,
    used_percent: f64,
    window_seconds: i64,
    resets_at: Option<i64>,
) -> LimitWindow {
    LimitWindow {
        key: key.into(),
        label: i18n::window_label(window_seconds),
        used_percent,
        resets_at,
        period_seconds: (window_seconds > 0).then_some(window_seconds),
        group: None,
    }
}

fn parse_window(key: &str, v: &Value) -> Option<LimitWindow> {
    Some(limit_window(
        key,
        v.get("used_percent")?.as_f64()?,
        v.get("limit_window_seconds").and_then(Value::as_i64).unwrap_or(0),
        v.get("reset_at").and_then(Value::as_i64),
    ))
}

pub fn plan_label(raw: &str) -> String {
    match raw {
        "plus" => "Plus".into(),
        "pro" => "Pro".into(),
        "team" => "Team".into(),
        "business" => "Business".into(),
        "enterprise" => "Enterprise".into(),
        "edu" => "Edu".into(),
        "free" => "Free".into(),
        other => other.to_string(),
    }
}

pub async fn fetch(account: &Account) -> ProviderStatus {
    let status = ProviderStatus::new(account);

    let Some((token, chatgpt_account)) = read_auth() else {
        return status.needs_login(t(
            "Codex 앱에서 ChatGPT 계정으로 로그인해 주세요.",
            "Sign in to the Codex app with your ChatGPT account.",
        ));
    };

    let mut req = client()
        .get(USAGE_URL)
        .bearer_auth(token)
        .header("accept", "application/json");
    if let Some(id) = chatgpt_account {
        req = req.header("chatgpt-account-id", id);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) if e.is_timeout() => return status.error(i18n::timed_out()),
        Err(_) => return status.error(i18n::offline()),
    };

    let code = resp.status();
    if code == reqwest::StatusCode::UNAUTHORIZED || code == reqwest::StatusCode::FORBIDDEN {
        return status.needs_login(t(
            "로그인이 만료되었습니다. Codex 앱을 한 번 실행해 주세요.",
            "The login has expired. Open the Codex app once.",
        ));
    }
    if !code.is_success() {
        return status.error(i18n::bad_reply("ChatGPT", &code.as_u16().to_string()));
    }

    let json: Value = match resp.json().await {
        Ok(j) => j,
        Err(_) => return status.error(i18n::unreadable()),
    };

    let mut windows = Vec::new();
    if let Some(rl) = json.get("rate_limit") {
        for (key, field) in [("primary", "primary_window"), ("secondary", "secondary_window")] {
            if let Some(w) = rl.get(field).and_then(|v| parse_window(key, v)) {
                windows.push(w);
            }
        }
    }
    if windows.is_empty() {
        return status.error(i18n::no_limits());
    }

    let plan = json.get("plan_type").and_then(Value::as_str).map(plan_label);
    status.ok(plan, windows)
}
