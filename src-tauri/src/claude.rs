//! Claude plan usage.
//!
//! The Claude desktop app keeps no login on disk that we could reuse, so the
//! user signs in to claude.ai in a window of ours. The hidden window is parked
//! on a tiny JSON endpoint instead of the full web app to stay light.

use crate::{
    bridge,
    i18n::{self, t},
    model::LimitWindow,
    web::{Outcome, Spec},
};
use serde::Deserialize;
use tauri::Url;

pub static SPEC: Spec = Spec {
    host: "claude.ai",
    load_url: "https://claude.ai/api/organizations",
    login_url: "https://claude.ai/login",
    sign_in_host: None,
    script: FETCH_SCRIPT,
    init_script: None,
    heavy: false,
    cookie_urls: &["https://claude.ai"],
    needs_login: ("Claude 계정 로그인이 필요합니다.", "Sign in to your Claude account."),
    parse,
};

/// Sends back only plan capabilities and usage percentages — no account names
/// or e-mail addresses.
const FETCH_SCRIPT: &str = r#"
(async () => {
  const out = {};
  const get = (path) =>
    fetch(path, { credentials: 'include', headers: { accept: 'application/json' } });
  const denied = (r) => r.status === 401 || r.status === 403;
  try {
    const r = await get('/api/organizations');
    if (denied(r)) {
      out.needsLogin = true;
    } else if (!r.ok) {
      out.error = 'HTTP ' + r.status;
    } else {
      const body = await r.json();
      const orgs = Array.isArray(body) ? body : [];
      const caps = (o) => (Array.isArray(o.capabilities) ? o.capabilities : []);
      const org =
        orgs.find((o) => caps(o).some((c) => c === 'claude_max' || c === 'claude_pro')) ||
        orgs.find((o) => caps(o).includes('chat')) ||
        orgs[0];
      if (!org) {
        out.error = 'no organization';
      } else {
        out.capabilities = caps(org).filter((c) => typeof c === 'string');
        out.tier = typeof org.rate_limit_tier === 'string' ? org.rate_limit_tier : null;
        const u = await get('/api/organizations/' + org.uuid + '/usage');
        if (denied(u)) {
          out.needsLogin = true;
        } else if (!u.ok) {
          out.error = 'HTTP ' + u.status;
        } else {
          const usage = await u.json();
          out.windows = Object.entries(usage)
            .filter(([, v]) => v && typeof v === 'object' && typeof v.utilization === 'number')
            .map(([key, v]) => ({
              key,
              usedPercent: v.utilization,
              resetsAt: v.resets_at ? Math.floor(Date.parse(v.resets_at) / 1000) : null,
            }));
        }
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
    capabilities: Vec<String>,
    tier: Option<String>,
    windows: Vec<RawWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWindow {
    key: String,
    used_percent: f64,
    resets_at: Option<i64>,
}

/// (sort order, label) for a usage key, or `None` for keys we do not show.
fn window_label(key: &str) -> Option<(u8, String)> {
    match key {
        "five_hour" => Some((0, t("5시간", "5-hour").into())),
        "seven_day" => Some((1, t("주간", "Weekly").into())),
        "seven_day_oauth_apps" => None,
        _ => {
            let model = key.strip_prefix("seven_day_")?;
            let mut chars = model.chars();
            let name: String = match chars.next() {
                Some(c) => c.to_uppercase().chain(chars).collect(),
                None => return None,
            };
            Some((2, format!("{} · {}", t("주간", "Weekly"), name.replace('_', " "))))
        }
    }
}

fn plan_label(capabilities: &[String], tier: Option<&str>) -> Option<String> {
    let has = |name: &str| capabilities.iter().any(|c| c == name);
    let tier = tier.unwrap_or("");
    if has("claude_max") {
        Some(if tier.contains("20x") {
            "Max 20x".into()
        } else if tier.contains("5x") {
            "Max 5x".into()
        } else {
            "Max".into()
        })
    } else if has("claude_pro") {
        Some("Pro".into())
    } else if has("raven") {
        Some("Team".into())
    } else {
        None
    }
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
            i18n::bad_reply("Claude", &err)
        });
    }

    let mut windows: Vec<(u8, LimitWindow)> = report
        .windows
        .into_iter()
        .filter_map(|w| {
            let (order, label) = window_label(&w.key)?;
            Some((
                order,
                LimitWindow {
                    key: w.key,
                    label,
                    used_percent: w.used_percent,
                    resets_at: w.resets_at,
                    period_seconds: Some(if order == 0 { 18_000 } else { 604_800 }),
                    group: None,
                },
            ))
        })
        .collect();
    if windows.is_empty() {
        return Outcome::Error(i18n::no_limits().into());
    }
    windows.sort_by_key(|(order, _)| *order);

    Outcome::Ok {
        plan: plan_label(&report.capabilities, report.tier.as_deref()),
        windows: windows.into_iter().map(|(_, w)| w).collect(),
    }
}
