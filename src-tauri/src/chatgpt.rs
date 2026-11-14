//! Codex / ChatGPT plan usage for accounts signed in on the web.
//!
//! The built-in Codex account is read from the Codex app's login on this PC
//! (see `codex.rs`). Additional accounts sign in to chatgpt.com in a window of
//! ours; the hidden window then asks the same usage endpoint with that web
//! session. It is parked on the small session endpoint rather than the web app.

use crate::{
    bridge, codex, i18n,
    web::{Outcome, Spec},
};
use serde::Deserialize;
use tauri::Url;

pub static SPEC: Spec = Spec {
    host: "chatgpt.com",
    load_url: "https://chatgpt.com/api/auth/session",
    login_url: "https://chatgpt.com/auth/login",
    sign_in_host: None,
    script: FETCH_SCRIPT,
    init_script: None,
    heavy: false,
    cookie_urls: &["https://chatgpt.com", "https://auth.openai.com"],
    needs_login: ("ChatGPT 계정 로그인이 필요합니다.", "Sign in to your ChatGPT account."),
    parse,
};

/// The session token is used inside the page only; just plan and percentages
/// are sent back.
const FETCH_SCRIPT: &str = r#"
(async () => {
  const out = {};
  const denied = (r) => r.status === 401 || r.status === 403;
  try {
    const s = await fetch('/api/auth/session', {
      credentials: 'include',
      headers: { accept: 'application/json' },
    });
    const session = s.ok ? await s.json().catch(() => null) : null;
    const token = session && typeof session.accessToken === 'string' ? session.accessToken : '';
    if (denied(s) || (s.ok && !token)) {
      out.needsLogin = true;
    } else if (!s.ok) {
      out.error = 'HTTP ' + s.status;
    } else {
      const u = await fetch('/backend-api/wham/usage', {
        credentials: 'include',
        headers: { accept: 'application/json', authorization: 'Bearer ' + token },
      });
      if (u.status === 401) {
        out.needsLogin = true;
      } else if (!u.ok) {
        out.error = 'HTTP ' + u.status;
      } else {
        const usage = await u.json();
        const limit = usage.rate_limit || {};
        out.plan = typeof usage.plan_type === 'string' ? usage.plan_type : null;
        out.windows = [
          ['primary', limit.primary_window],
          ['secondary', limit.secondary_window],
        ]
          .filter(([, w]) => w && typeof w.used_percent === 'number')
          .map(([key, w]) => ({
            key,
            usedPercent: w.used_percent,
            windowSeconds: typeof w.limit_window_seconds === 'number' ? w.limit_window_seconds : 0,
            resetsAt: typeof w.reset_at === 'number' ? w.reset_at : null,
          }));
      }
    }
  } catch (e) {
    out.error = 'network';
  }
  __SEND__
})();
"#;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Report {
    needs_login: bool,
    error: Option<String>,
    plan: Option<String>,
    windows: Vec<RawWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWindow {
    key: String,
    used_percent: f64,
    window_seconds: i64,
    resets_at: Option<i64>,
}

fn parse(url: &Url) -> Outcome {
    let Some(report) = bridge::parse_report::<Report>(url) else {
        return Outcome::Error(i18n::unreadable().into());
    };
    if report.needs_login {
        return Outcome::NeedsLogin;
    }
    if let Some(err) = report.error {
        return Outcome::Error(if err == "network" {
            i18n::offline().into()
        } else {
            i18n::bad_reply("ChatGPT", &err)
        });
    }
    if report.windows.is_empty() {
        return Outcome::Error(i18n::no_limits().into());
    }
    Outcome::Ok {
        plan: report.plan.as_deref().map(codex::plan_label),
        windows: report
            .windows
            .into_iter()
            .map(|w| codex::limit_window(&w.key, w.used_percent, w.window_seconds, w.resets_at))
            .collect(),
    }
}
