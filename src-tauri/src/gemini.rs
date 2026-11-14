//! Gemini app usage limits.
//!
//! Gemini shows the plan's "Current usage" (5-hour) and "Weekly limit"
//! percentages on gemini.google.com/usage. There is no API for it, so the
//! hidden window opens that page (in English, so the text is predictable) and
//! reads the two rows. The page is a full web app, hence `heavy`.

use crate::{
    bridge,
    i18n::{self, t},
    model::LimitWindow,
    web::{Outcome, Spec},
};
use serde::Deserialize;
use tauri::Url;

pub static SPEC: Spec = Spec {
    host: "gemini.google.com",
    load_url: "https://gemini.google.com/usage?hl=en",
    login_url:
        "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fgemini.google.com%2Fusage%3Fhl%3Den",
    // Signed-out visits are bounced to Google's sign-in page.
    sign_in_host: Some("accounts.google.com"),
    script: READ_SCRIPT,
    init_script: None,
    heavy: true,
    cookie_urls: &[
        "https://gemini.google.com",
        "https://accounts.google.com",
        "https://www.google.com",
    ],
    needs_login: ("Google 계정 로그인이 필요합니다.", "Sign in to your Google account."),
    parse,
};

/// Runs in the usage page. Reports only row labels, percentages and reset
/// times — nothing that identifies the account.
const READ_SCRIPT: &str = r#"
(async () => {
  if (window.__ttReading) return;
  window.__ttReading = true;
  const out = {};
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const MONTHS = ['jan','feb','mar','apr','may','jun','jul','aug','sep','oct','nov','dec'];

  // "Resets at 2:09 PM" / "Resets tomorrow at 2:09 PM" / "Resets Nov 17 at 3:09 PM"
  const parseReset = (text) => {
    const m = text.match(
      /Resets\s+(?:(tomorrow)\s+at\s+|([A-Za-z]{3,9})\.?\s+(\d{1,2})(?:,\s*(\d{4}))?\s+at\s+|at\s+)(\d{1,2}):(\d{2})\s*([AP]M)/i
    );
    if (!m) return null;
    const hour = (Number(m[5]) % 12) + (/pm/i.test(m[7]) ? 12 : 0);
    const minute = Number(m[6]);
    const now = new Date();
    let d;
    if (m[2]) {
      const month = MONTHS.indexOf(m[2].slice(0, 3).toLowerCase());
      if (month < 0) return null;
      const year = m[4] ? Number(m[4]) : now.getFullYear();
      d = new Date(year, month, Number(m[3]), hour, minute);
      if (!m[4] && d.getTime() < now.getTime() - 86400000) {
        d = new Date(year + 1, month, Number(m[3]), hour, minute);
      }
    } else {
      d = new Date(now.getFullYear(), now.getMonth(), now.getDate(), hour, minute);
      if (m[1] || d.getTime() < now.getTime() - 60000) d.setDate(d.getDate() + 1);
    }
    return Math.floor(d.getTime() / 1000);
  };

  const readRows = () =>
    Array.from(document.querySelectorAll('li'))
      .filter((li) => !li.querySelector('li') && /\d+(?:\.\d+)?\s*%\s*used/i.test(li.innerText))
      .map((li) => {
        const text = li.innerText;
        const flat = text.replace(/\s+/g, ' ');
        return {
          label: text.split('\n')[0].trim().slice(0, 40),
          usedPercent: Number(flat.match(/(\d+(?:\.\d+)?)\s*%\s*used/i)[1]),
          resetsAt: parseReset(flat),
        };
      });

  const signedOut = () =>
    Array.from(document.querySelectorAll('a')).some(
      (a) => /accounts\.google\.com\/(ServiceLogin|v3\/signin|signin)/.test(a.href) ||
        /^sign in$/i.test((a.innerText || '').trim())
    );

  try {
    const started = Date.now();
    for (;;) {
      const rows = readRows();
      if (rows.length) { out.windows = rows; break; }
      const waited = Date.now() - started;
      if (waited > 3000 && signedOut()) { out.needsLogin = true; break; }
      if (waited > 25000) { out.error = 'no-data'; break; }
      await sleep(500);
    }
  } catch (e) {
    out.error = 'script';
  }
  __SEND__
})();
"#;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Report {
    needs_login: bool,
    error: Option<String>,
    windows: Vec<RawWindow>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawWindow {
    label: String,
    used_percent: f64,
    resets_at: Option<i64>,
}

fn to_window(raw: RawWindow) -> LimitWindow {
    let lower = raw.label.to_lowercase();
    let (key, label, period_seconds) = if lower.contains("week") {
        ("seven_day".to_string(), t("주간", "Weekly").to_string(), Some(604_800))
    } else if lower.contains("current") {
        ("five_hour".to_string(), t("5시간", "5-hour").to_string(), Some(18_000))
    } else {
        (lower.replace(' ', "_"), raw.label.clone(), None)
    };
    LimitWindow {
        key,
        label,
        used_percent: raw.used_percent,
        resets_at: raw.resets_at,
        period_seconds,
        group: None,
    }
}

fn parse(url: &Url) -> Outcome {
    let Some(report) = bridge::parse_report::<Report>(url) else {
        return Outcome::Error(i18n::unreadable().into());
    };
    if report.needs_login {
        return Outcome::NeedsLogin;
    }
    if report.windows.is_empty() {
        return Outcome::Error(
            match report.error.as_deref() {
                Some("no-data") | None => t(
                    "사용량 화면에서 한도 정보를 찾지 못했습니다.",
                    "No limits found on the usage page.",
                ),
                Some(_) => t(
                    "사용량 화면을 읽는 중 오류가 났습니다.",
                    "Reading the usage page failed.",
                ),
            }
            .into(),
        );
    }
    let mut windows: Vec<LimitWindow> = report.windows.into_iter().map(to_window).collect();
    // Short window first, like the other providers.
    windows.sort_by_key(|w| match w.key.as_str() {
        "five_hour" => 0,
        "seven_day" => 1,
        _ => 2,
    });
    Outcome::Ok {
        plan: None,
        windows,
    }
}
