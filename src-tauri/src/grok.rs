//! Grok (SuperGrok / X Premium) plan usage.
//!
//! Since mid-2026, paid Grok plans share one weekly usage pool, shown as a
//! percentage under Settings → Usage on grok.com (also at grok.com/dashboard).
//! There is no published API for it. The hidden window opens the dashboard
//! and looks for the pool in two places: the usage data the page itself
//! loaded, and failing that the text on the page.
//!
//! We send no requests of our own to grok.com: `WATCH_SCRIPT` keeps a copy of
//! the usage data the dashboard fetches anyway, so a refresh costs no more
//! than the user opening the dashboard once.
//!
//! This is written without a Grok account to try it on, so it looks for the
//! numbers by their shape rather than by exact names.

use crate::{
    bridge,
    i18n::{self, t},
    model::LimitWindow,
    web::{Outcome, Spec},
};
use serde::Deserialize;
use tauri::Url;

pub static SPEC: Spec = Spec {
    host: "grok.com",
    load_url: "https://grok.com/dashboard",
    login_url: "https://accounts.x.ai/sign-in?redirect=grok-com&return_to=%2Fdashboard",
    // Signed-out visits are sent to xAI's sign-in page.
    sign_in_host: Some("accounts.x.ai"),
    script: READ_SCRIPT,
    init_script: Some(WATCH_SCRIPT),
    heavy: true,
    cookie_urls: &["https://grok.com", "https://accounts.x.ai", "https://x.ai"],
    needs_login: ("Grok 계정 로그인이 필요합니다.", "Sign in to your Grok account."),
    parse,
};

/// Runs before the dashboard's own scripts. Keeps the JSON answers to the
/// requests the page itself makes for usage data in `window.__ttSeen`, and
/// notes in `window.__ttDenied` when one of them was refused. It changes no
/// request and makes none.
const WATCH_SCRIPT: &str = r#"
(() => {
  if (location.host !== 'grok.com' || window.__ttWatch) return;
  window.__ttWatch = true;
  window.__ttSeen = [];
  window.__ttDenied = false;
  const wanted = (u) => {
    try {
      const url = new URL(String(u), location.href);
      return url.host === 'grok.com' && /usage|limit|quota|billing|subscription|credit|pool/i.test(url.pathname);
    } catch (e) { return false; }
  };
  const keep = (data) => {
    if (data && typeof data === 'object' && window.__ttSeen.length < 50) window.__ttSeen.push(data);
  };
  const seeStatus = (code) => { if (code === 401 || code === 403) window.__ttDenied = true; };
  const isJson = (type) => /json/.test(type || '');

  const pageFetch = window.fetch;
  window.fetch = function (input) {
    const answer = pageFetch.apply(this, arguments);
    try {
      const u = input && typeof input === 'object' && 'url' in input ? input.url : input;
      if (wanted(u)) {
        answer.then((r) => {
          seeStatus(r.status);
          if (r.ok && isJson(r.headers.get('content-type'))) r.clone().json().then(keep, () => {});
        }, () => {});
      }
    } catch (e) {}
    return answer;
  };

  const open = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    if (wanted(url)) {
      this.addEventListener('load', () => {
        try {
          seeStatus(this.status);
          if (this.status < 200 || this.status >= 300 || !isJson(this.getResponseHeader('content-type'))) return;
          keep(this.responseType === 'json' ? this.response : JSON.parse(this.responseText));
        } catch (e) {}
      });
    }
    return open.apply(this, arguments);
  };
})();
"#;

/// Runs in the dashboard. Reports only percentages, reset times and short
/// row labels — nothing that identifies the account.
const READ_SCRIPT: &str = r#"
(async () => {
  const out = {};
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const MONTHS = ['jan','feb','mar','apr','may','jun','jul','aug','sep','oct','nov','dec'];

  // Epoch seconds from an ISO string, or from epoch seconds / milliseconds.
  const toEpoch = (v) => {
    if (typeof v === 'number' && isFinite(v) && v > 1e9) return Math.floor(v > 1e12 ? v / 1000 : v);
    if (typeof v === 'string' && /^\d{4}-\d{2}-\d{2}T/.test(v)) {
      const ms = Date.parse(v);
      return isNaN(ms) ? null : Math.floor(ms / 1000);
    }
    return null;
  };

  // A usage share from the data the page loaded: an object with a used
  // percentage (or used and limit) and, near it, a reset time.
  const fromJson = (root) => {
    const found = [];
    const walk = (node, path, depth) => {
      if (!node || typeof node !== 'object' || depth > 8) return;
      if (Array.isArray(node)) { node.forEach((n, i) => walk(n, path + '[' + i + ']', depth + 1)); return; }
      const keys = Object.keys(node);
      let pct = null;
      for (const k of keys) {
        const v = node[k];
        if (typeof v !== 'number' || !isFinite(v)) continue;
        if (/(used|usage|consumed|utili[sz]ation).*(percent|pct)|percent.*used|^utili[sz]ation$/i.test(k)) {
          pct = v <= 1 && !/percent|pct/i.test(k) ? v * 100 : v;
        } else if (/(used|usage|consumed).*(fraction|ratio)/i.test(k)) {
          pct = v * 100;
        } else if (/(remaining|left).*(fraction|ratio)/i.test(k)) {
          pct = (1 - v) * 100;
        } else if (/(remaining|left).*(percent|pct)/i.test(k)) {
          pct = 100 - v;
        }
      }
      if (pct === null) {
        const used = keys.find((k) => /^(used|usage|consumed|spent)(Amount|Units|Credits|Compute)?$/i.test(k) && typeof node[k] === 'number');
        const limit = keys.find((k) => /^(limit|total|quota|allowance|max)(Amount|Units|Credits|Compute)?$/i.test(k) && typeof node[k] === 'number');
        if (used && limit && node[limit] > 0) pct = (node[used] / node[limit]) * 100;
      }
      if (pct !== null && pct >= 0 && pct <= 100) {
        let resetsAt = null;
        for (const k of keys) {
          if (/reset|renew|refresh|period.?end|ends?_?at|window.?end/i.test(k)) {
            resetsAt = toEpoch(node[k]);
            if (resetsAt) break;
          }
        }
        found.push({ path, usedPercent: pct, resetsAt });
      }
      for (const k of keys) walk(node[k], path + '.' + k, depth + 1);
    };
    walk(root, '', 0);
    return found;
  };

  // "Resets Nov 20" / "Resets on Nov 20 at 3:00 PM" / "Resets in 3 days"
  const parseReset = (text) => {
    let m = text.match(/Resets?\s+in\s+(\d+)\s*(day|hour|minute|min|d|h|m)/i);
    if (m) {
      const n = Number(m[1]);
      const unit = m[2].toLowerCase();
      const s = unit.startsWith('d') ? n * 86400 : unit.startsWith('h') ? n * 3600 : n * 60;
      return Math.floor(Date.now() / 1000) + s;
    }
    m = text.match(/Resets?\s+(?:on\s+)?([A-Za-z]{3,9})\.?\s+(\d{1,2})(?:,\s*(\d{4}))?(?:\s+at\s+(\d{1,2}):(\d{2})\s*([AP]M)?)?/i);
    if (!m) return null;
    const month = MONTHS.indexOf(m[1].slice(0, 3).toLowerCase());
    if (month < 0) return null;
    const now = new Date();
    let hour = m[4] ? Number(m[4]) % 12 + (/pm/i.test(m[6] || '') ? 12 : 0) : 0;
    if (m[4] && !m[6]) hour = Number(m[4]);
    const year = m[3] ? Number(m[3]) : now.getFullYear();
    let d = new Date(year, month, Number(m[2]), hour, m[5] ? Number(m[5]) : 0);
    if (!m[3] && d.getTime() < now.getTime() - 86400000) d = new Date(year + 1, month, Number(m[2]), hour, m[5] ? Number(m[5]) : 0);
    return Math.floor(d.getTime() / 1000);
  };

  // The pool as the page shows it: the smallest block with a percentage and
  // a word about usage.
  const fromPage = () => {
    const blocks = Array.from(document.querySelectorAll('section, li, div'))
      .filter((el) => {
        const text = el.innerText || '';
        return text.length < 400 && /\d+(?:\.\d+)?\s*%/.test(text) && /usage|used|weekly|pool|limit/i.test(text);
      })
      .filter((el, _, all) => !all.some((o) => o !== el && el.contains(o)));
    return blocks.map((el) => {
      const flat = el.innerText.replace(/\s+/g, ' ');
      const m = flat.match(/(\d+(?:\.\d+)?)\s*%\s*(used|left|remaining)?/i);
      let pct = Number(m[1]);
      if (m[2] && /left|remaining/i.test(m[2])) pct = 100 - pct;
      return {
        label: el.innerText.split('\n')[0].trim().slice(0, 40),
        usedPercent: pct,
        resetsAt: parseReset(flat),
      };
    });
  };

  const signedOut = () =>
    location.host !== 'grok.com' ||
    Array.from(document.querySelectorAll('a[href]')).some((a) => /accounts\.x\.ai\/sign-in/.test(a.href));

  try {
    const started = Date.now();
    let checked = 0;
    for (;;) {
      const waited = Date.now() - started;
      // Usage data the page itself loaded (see WATCH_SCRIPT).
      const seen = window.__ttSeen || [];
      for (; checked < seen.length; checked++) {
        const rows = fromJson(seen[checked]);
        if (rows.length) { out.windows = rows.slice(0, 1).map((w) => ({ ...w, label: 'weekly' })); out.source = 'data'; break; }
      }
      if (!out.windows && window.__ttDenied) out.needsLogin = true;
      if (out.windows || out.needsLogin) break;
      if (waited > 6000) {
        const rows = fromPage();
        if (rows.length) { out.windows = rows.slice(0, 3); out.source = 'page'; break; }
      }
      if (waited > 4000 && signedOut()) { out.needsLogin = true; break; }
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

fn to_window(index: usize, raw: RawWindow) -> LimitWindow {
    let lower = raw.label.to_lowercase();
    // The first row is the shared pool, which is weekly.
    let weekly = index == 0 || lower.contains("week");
    LimitWindow {
        key: if weekly {
            "seven_day".into()
        } else {
            format!("grok_{index}")
        },
        label: if weekly {
            t("주간", "Weekly").into()
        } else {
            raw.label
        },
        used_percent: raw.used_percent.clamp(0.0, 100.0),
        resets_at: raw.resets_at,
        period_seconds: weekly.then_some(604_800),
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
    Outcome::Ok {
        plan: None,
        windows: report
            .windows
            .into_iter()
            .enumerate()
            .map(|(i, w)| to_window(i, w))
            .collect(),
    }
}
