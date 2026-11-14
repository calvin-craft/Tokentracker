//! Shared plumbing for providers that are read through a signed-in webview.
//!
//! A script running in the provider's page reports back by navigating to
//! `https://BRIDGE_HOST/r#<hex json>`. The navigation is intercepted and
//! cancelled on the Rust side, so remote pages never get IPC access.

use serde::de::DeserializeOwned;
use tauri::{Url, WebviewWindow};

pub const BRIDGE_HOST: &str = "tt-bridge.invalid";

pub fn is_report(url: &Url) -> bool {
    url.host_str() == Some(BRIDGE_HOST)
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Decode the JSON payload carried in a report URL's fragment.
pub fn parse_report<T: DeserializeOwned>(url: &Url) -> Option<T> {
    let bytes = decode_hex(url.fragment()?)?;
    serde_json::from_slice(&bytes).ok()
}

/// JS snippet that sends the object in variable `out` back over the bridge.
/// Provider scripts carry a `__SEND__` placeholder that is replaced with this.
pub const SEND_OUT_JS: &str = r#"
  {
    let hex = '';
    for (const b of new TextEncoder().encode(JSON.stringify(out))) {
      hex += b.toString(16).padStart(2, '0');
    }
    location.href = 'https://tt-bridge.invalid/r#' + hex;
  }
"#;

/// Sign out by deleting the cookies for `urls` from the cookie store that
/// `window` uses. Built-in accounts share one store, so it must not be wiped.
pub fn clear_cookies(window: &WebviewWindow, urls: &[&str]) {
    for url in urls {
        let Ok(url) = url.parse::<Url>() else { continue };
        if let Ok(cookies) = window.cookies_for_url(url) {
            for cookie in cookies {
                let _ = window.delete_cookie(cookie);
            }
        }
    }
}
