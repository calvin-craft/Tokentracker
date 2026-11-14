//! Accounts that are read through a signed-in webview.
//!
//! The user signs in once in a visible window. After that a hidden window that
//! shares the account's cookie store loads a page of the provider, runs the
//! provider's script there, and receives the result over the bridge.
//!
//! Each provider describes itself with a `Spec`; everything else — windows,
//! cookie stores, timeouts, sign-in and sign-out — is the same for all.

use crate::{
    bridge, chatgpt, claude, gemini, grok, i18n,
    model::{LimitWindow, ProviderStatus},
    publish,
    settings::{self, Account},
    Shared,
};
use std::time::{Duration, Instant};
use tauri::{
    webview::PageLoadEvent, AppHandle, Manager, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Wry,
};

const BLANK_URL: &str = "about:blank";

/// Browser options for every window of ours (WebView2 only; the webviews of
/// macOS and Linux take none). A small widget and a few hidden
/// pages need no graphics card: drawing them in software saves the browser's
/// GPU process a few hundred MB. The first part is the webview's default.
/// Windows that share a cookie store must all be given the same options;
/// the widget gets them from `tauri.conf.json`.
pub const BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --disable-gpu --disable-gpu-compositing";
const NO_REPLY_AFTER: Duration = Duration::from_secs(60);

/// What a provider's script reported.
pub enum Outcome {
    NeedsLogin,
    Error(String),
    Ok {
        plan: Option<String>,
        windows: Vec<LimitWindow>,
    },
}

pub struct Spec {
    /// Host on which the script may run.
    pub host: &'static str,
    /// Page the hidden window loads for every refresh.
    pub load_url: &'static str,
    /// Page the sign-in window opens.
    pub login_url: &'static str,
    /// Landing on this host instead of `host` means "signed out".
    pub sign_in_host: Option<&'static str>,
    /// Script with a `__SEND__` placeholder (see `bridge::SEND_OUT_JS`).
    pub script: &'static str,
    /// Script that runs before the page's own, on every page of the hidden
    /// window; it must check the host itself.
    pub init_script: Option<&'static str>,
    /// The page is a full web app; leave it between refreshes to free memory.
    pub heavy: bool,
    /// Cookies to delete on sign-out.
    pub cookie_urls: &'static [&'static str],
    /// (Korean, English) message shown while signed out.
    pub needs_login: (&'static str, &'static str),
    /// Turns a bridge report URL into an outcome.
    pub parse: fn(&Url) -> Outcome,
}

pub fn spec_for(account: &Account) -> Option<&'static Spec> {
    match account.provider.as_str() {
        "codex" if account.is_local() => None,
        "codex" => Some(&chatgpt::SPEC),
        "claude" => Some(&claude::SPEC),
        "gemini" => Some(&gemini::SPEC),
        "grok" => Some(&grok::SPEC),
        _ => None,
    }
}

fn bg_label(account: &Account) -> String {
    format!("{}-bg", account.id)
}

fn login_label(account: &Account) -> String {
    format!("{}-login", account.id)
}

fn parse_url(url: &str) -> Url {
    url.parse().expect("valid url")
}

fn needs_login_text(spec: &Spec) -> &'static str {
    i18n::t(spec.needs_login.0, spec.needs_login.1)
}

fn to_status(account: &Account, spec: &Spec, outcome: Outcome) -> ProviderStatus {
    let status = ProviderStatus::new(account);
    match outcome {
        Outcome::NeedsLogin => status.needs_login(needs_login_text(spec)),
        Outcome::Error(message) => status.error(message),
        Outcome::Ok { plan, windows } => status.ok(plan, windows),
    }
}

/// Record a result, then let go of whatever the hidden window is holding.
fn finish(app: &AppHandle, account: &Account, spec: &'static Spec, outcome: Outcome) {
    let state = app.state::<Shared>();
    state.pending.lock().unwrap().remove(&account.id);

    let signed_in = matches!(outcome, Outcome::Ok { .. });
    publish(app, to_status(account, spec, outcome));
    if signed_in {
        if let Some(w) = app.get_webview_window(&login_label(account)) {
            let _ = w.close();
        }
    }

    // An added account has a cookie store, and so a browser process, of its
    // own: close its hidden window between refreshes. A built-in account only
    // needs to leave a heavy page. Deferred, because we may be running inside
    // this very window's navigation callback.
    let own_store = !account.is_builtin();
    if !own_store && !spec.heavy {
        return;
    }
    let app = app.clone();
    let account = account.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let state = app.state::<Shared>();
        if state.pending.lock().unwrap().contains_key(&account.id) {
            return; // a new refresh has started
        }
        let Some(window) = app.get_webview_window(&bg_label(&account)) else {
            return;
        };
        if own_store && !is_login_open(&app, &account) {
            let _ = window.close();
        } else if spec.heavy {
            let _ = window.navigate(parse_url(BLANK_URL));
        }
    });
}

/// Gives a window the account's own cookie store, if it has one.
trait WithProfile {
    fn with_profile(self, app: &AppHandle, account: &Account) -> Self;
}

impl WithProfile for WebviewWindowBuilder<'_, Wry, AppHandle> {
    fn with_profile(self, app: &AppHandle, account: &Account) -> Self {
        #[cfg(target_os = "macos")]
        if let Some(id) = settings::profile_store_id(account) {
            return self.data_store_identifier(id);
        }
        match settings::profile_dir(app, account) {
            Some(dir) => self.data_directory(dir),
            None => self,
        }
    }
}

/// The account's hidden window, created on first use (which starts a load).
fn background_window(
    app: &AppHandle,
    account: &Account,
    spec: &'static Spec,
) -> Option<(WebviewWindow, bool)> {
    let label = bg_label(account);
    if let Some(w) = app.get_webview_window(&label) {
        return Some((w, false));
    }

    // Cancelling the report navigation makes the webview announce the current
    // page as loaded once more. Without the guard that would run the script
    // again, which reports again, and so on several times a second.
    let script = format!(
        "if (!window.__ttRan) {{ window.__ttRan = true; {} }}",
        spec.script.replace("__SEND__", bridge::SEND_OUT_JS)
    );
    let (nav_app, nav_account) = (app.clone(), account.clone());
    let (load_app, load_account) = (app.clone(), account.clone());

    let builder =
        WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url(spec.load_url)))
            .title("TokenTracker")
            .additional_browser_args(BROWSER_ARGS)
            .inner_size(900.0, 700.0)
            .visible(false)
            // Created on every refresh for an added account; taking focus
            // would send a widget the user just clicked back behind windows.
            .focused(false)
            .skip_taskbar(true)
            .on_navigation(move |url| {
                if bridge::is_report(url) {
                    finish(&nav_app, &nav_account, spec, (spec.parse)(url));
                    false
                } else {
                    true
                }
            })
            .on_page_load(move |window, payload| {
                if !matches!(payload.event(), PageLoadEvent::Finished) {
                    return;
                }
                // Only a load that a refresh asked for is read.
                let wanted = load_app
                    .state::<Shared>()
                    .pending
                    .lock()
                    .unwrap()
                    .contains_key(&load_account.id);
                if !wanted {
                    return;
                }
                let host = payload.url().host_str();
                if host == Some(spec.host) {
                    let _ = window.eval(script.as_str());
                } else if host.is_some() && host == spec.sign_in_host {
                    finish(&load_app, &load_account, spec, Outcome::NeedsLogin);
                }
            });
    let builder = match spec.init_script {
        Some(init) => builder.initialization_script(init),
        None => builder,
    };
    builder.with_profile(app, account).build().ok().map(|w| (w, true))
}

/// Kick off a refresh unless one is already running. The result arrives
/// asynchronously through `finish`.
pub fn request(app: &AppHandle, account: &Account) {
    let Some(spec) = spec_for(account) else { return };
    let state = app.state::<Shared>();
    let timed_out = {
        let mut pending = state.pending.lock().unwrap();
        let previous = pending.get(&account.id).copied();
        if previous.is_some_and(|since| since.elapsed() <= NO_REPLY_AFTER) {
            return;
        }
        pending.insert(account.id.clone(), Instant::now());
        previous.is_some()
    };
    if timed_out {
        let message = if i18n::is_english() {
            format!("No response from {}.", account.provider_name())
        } else {
            format!("{} 응답이 없습니다.", account.provider_name())
        };
        publish(app, ProviderStatus::new(account).error(message));
    }

    if let Some((window, created)) = background_window(app, account, spec) {
        if !created {
            let _ = window.navigate(parse_url(spec.load_url));
        }
    }
}

pub fn is_login_open(app: &AppHandle, account: &Account) -> bool {
    app.get_webview_window(&login_label(account)).is_some()
}

/// Show the provider's sign-in page. The user signs in there themselves.
pub fn open_login(app: &AppHandle, account: &Account) {
    let Some(spec) = spec_for(account) else { return };
    let label = login_label(account);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let title = format!(
        "{} {} — TokenTracker",
        account.display_name(),
        i18n::t("로그인", "Sign in")
    );
    let _ = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url(spec.login_url)))
        .title(title)
        .additional_browser_args(BROWSER_ARGS)
        .inner_size(520.0, 760.0)
        .with_profile(app, account)
        .build();
}

/// Forget the session this app holds for the account.
pub fn disconnect(app: &AppHandle, account: &Account) {
    let Some(spec) = spec_for(account) else { return };
    if let Some(w) = app.get_webview_window(&login_label(account)) {
        let _ = w.close();
    }
    if let Some((window, _)) = background_window(app, account, spec) {
        bridge::clear_cookies(&window, spec.cookie_urls);
    }
    publish(app, ProviderStatus::new(account).needs_login(needs_login_text(spec)));
}

/// Close an account's windows, e.g. because the account was removed.
pub fn close_windows(app: &AppHandle, account: &Account) {
    for label in [bg_label(account), login_label(account)] {
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.close();
        }
    }
    app.state::<Shared>()
        .pending
        .lock()
        .unwrap()
        .remove(&account.id);
}
