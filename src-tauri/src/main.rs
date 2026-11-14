#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod antigravity;
mod bridge;
mod chatgpt;
mod claude;
mod codex;
mod desktop;
mod gemini;
mod grok;
mod i18n;
mod model;
mod settings;
mod web;

use i18n::t;
use model::{ProviderStatus, Snapshot, STATE_ERROR, STATE_OK};
use settings::{Account, Settings};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use tokio::sync::Notify;

const WIDGET: &str = "widget";
const SETTINGS: &str = "settings";
const TRAY: &str = "main";
/// While a sign-in window is open, check often so it closes promptly.
const LOGIN_POLL: Duration = Duration::from_secs(3);
/// How often to check whether "Show desktop" (Win+D) is active.
const DESKTOP_POLL: Duration = Duration::from_millis(250);
/// How long the widget must sit still before it is pulled back on screen.
const CLAMP_DELAY: Duration = Duration::from_millis(200);
/// In wallpaper mode, how many desktop polls pass between checks that the
/// widget is still inside the desktop.
const EMBED_CHECK_POLLS: u32 = 12;
/// Argument with which an instance starts its replacement, followed by "="
/// and its own process id.
const RESTART_FLAG: &str = "--restarted";
/// How long a replacement waits for the instance that started it to end.
const RESTART_WAIT_MS: u32 = 5000;
/// How long after its window was destroyed the widget is opened again.
const REOPEN_DELAY: Duration = Duration::from_secs(2);

pub struct AppState {
    pub snapshot: Mutex<Snapshot>,
    pub settings: Mutex<Settings>,
    /// Highest alert threshold already notified, per "account:window".
    pub alert_levels: Mutex<HashMap<String, u8>>,
    /// When each web account's outstanding refresh was started.
    pub pending: Mutex<HashMap<String, Instant>>,
    /// Wakes the poll loop for an immediate refresh.
    pub wake: Notify,
    /// True while "Show desktop" has raised the desktop over normal windows.
    pub desktop_shown: AtomicBool,
    /// True while the widget has been brought forward and has not yet lost
    /// focus again.
    pub peeking: AtomicBool,
    /// Set when the app quits or restarts, so the closing widget is not
    /// opened again.
    pub exiting: AtomicBool,
    /// Bumped on every widget move or resize; debounces `schedule_clamp`.
    pub clamp_seq: AtomicU64,
}

pub type Shared = Arc<AppState>;

/// Store an account's new status, tell the UI, and raise alerts.
pub fn publish(app: &AppHandle, mut status: ProviderStatus) {
    let state = app.state::<Shared>();
    let snapshot = {
        let mut snap = state.snapshot.lock().unwrap();
        match snap.providers.iter_mut().find(|p| p.id == status.id) {
            Some(slot) => {
                // A transient failure keeps the last known numbers on screen.
                if status.state == STATE_ERROR && !slot.windows.is_empty() {
                    status.windows = slot.windows.clone();
                    status.plan = slot.plan.clone();
                    status.updated_at = slot.updated_at;
                }
                *slot = status.clone();
            }
            // The account was removed while its refresh was under way.
            None => return,
        }
        snap.clone()
    };
    let _ = app.emit("snapshot", &snapshot);
    if status.state == STATE_OK {
        check_alerts(app, &state, &status);
    }
}

fn check_alerts(app: &AppHandle, state: &AppState, status: &ProviderStatus) {
    let notify = state.settings.lock().unwrap().notify;
    let mut levels = state.alert_levels.lock().unwrap();
    for w in &status.windows {
        let level: u8 = if w.used_percent >= 95.0 {
            95
        } else if w.used_percent >= 80.0 {
            80
        } else {
            0
        };
        let prev = levels
            .insert(format!("{}:{}", status.id, w.key), level)
            .unwrap_or(0);
        if notify && level > prev {
            let account = if status.label.is_empty() {
                status.name.clone()
            } else {
                format!("{} ({})", status.name, status.label)
            };
            let limit = &w.label;
            let pct = w.used_percent.round() as i64;
            let title = if i18n::is_english() {
                format!("{account}: {limit} limit {pct}% used")
            } else {
                format!("{account} {limit} 한도 {pct}% 사용")
            };
            let _ = app
                .notification()
                .builder()
                .title(title)
                .body(if level >= 95 {
                    t("한도에 거의 도달했습니다.", "Almost at the limit.")
                } else {
                    t("한도의 80%를 넘었습니다.", "Over 80% of the limit.")
                })
                .show();
        }
    }
}

async fn poll_loop(app: AppHandle, state: Shared) {
    let mut last_full: Option<Instant> = None;
    let mut forced = true;
    loop {
        let settings = state.settings.lock().unwrap().clone();
        let interval = Duration::from_secs(u64::from(settings.refresh_minutes.max(1)) * 60);
        let due = forced || last_full.is_none_or(|t| t.elapsed() >= interval);
if due {
            last_full = Some(Instant::now());
        }

        let mut any_login = false;
        for account in settings.accounts.iter().filter(|a| a.shown) {
            if account.is_local() {
                if due {
                    let status = match account.provider.as_str() {
                        "antigravity" => {
                            antigravity::status(account, forced, settings.antigravity_launch).await
                        }
                        _ => codex::fetch(account).await,
                    };
                    publish(&app, status);
                }
            } else {
                let login_open = web::is_login_open(&app, account);
                any_login |= login_open;
                if due || login_open {
                    web::request(&app, account);
                }
            }
        }

        let wait = if any_login {
            LOGIN_POLL
        } else {
            let elapsed = last_full.map_or(Duration::ZERO, |t| t.elapsed());
            interval.saturating_sub(elapsed).max(Duration::from_secs(1))
        };
        forced = tokio::select! {
            _ = tokio::time::sleep(wait) => false,
            _ = state.wake.notified() => true,
        };
    }
}

/// Make the snapshot list the given accounts, in order, keeping what is
/// already known about each.
fn sync_snapshot(app: &AppHandle, state: &AppState, accounts: &[Account]) {
    let snapshot = {
        let mut snap = state.snapshot.lock().unwrap();
        let mut known: HashMap<String, ProviderStatus> = snap
            .providers
            .drain(..)
            .map(|p| (p.id.clone(), p))
            .collect();
        snap.providers = accounts
            .iter()
            .map(|account| match known.remove(&account.id) {
                Some(mut status) => {
                    status.label = account.label.clone();
                    status
                }
                None => ProviderStatus::new(account),
            })
            .collect();
        snap.clone()
    };
    let _ = app.emit("snapshot", &snapshot);
}

/// Close a removed account's windows and forget its login.
fn remove_account(app: &AppHandle, account: &Account) {
    if account.is_builtin() {
        // Built-in accounts share one cookie store, so only this account is
        // signed out of it. Cookie calls deadlock inside a synchronous
        // command, hence the task.
        let (app, account) = (app.clone(), account.clone());
        tauri::async_runtime::spawn(async move {
            web::disconnect(&app, &account);
            web::close_windows(&app, &account);
        });
        return;
    }
    web::close_windows(app, account);
    #[cfg(target_os = "macos")]
    if let Some(id) = settings::profile_store_id(account) {
        // A store still in use by a closing window cannot be removed yet.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if app.remove_data_store(id).await.is_ok() {
                    break;
                }
            }
        });
    }
    if let Some(dir) = settings::profile_dir(app, account) {
        // The browser releases the folder a moment after its windows close.
        std::thread::spawn(move || {
            for _ in 0..10 {
                std::thread::sleep(Duration::from_secs(1));
                if !dir.exists() || std::fs::remove_dir_all(&dir).is_ok() {
                    break;
                }
            }
        });
    }
}

#[cfg(windows)]
fn native_handle(w: &WebviewWindow) -> Option<desktop::Hwnd> {
    w.hwnd().ok().map(|h| h.0 as desktop::Hwnd)
}

#[cfg(not(windows))]
fn native_handle(_: &WebviewWindow) -> Option<desktop::Hwnd> {
    None
}

/// Take the widget out of the desktop if it is inside it. The window toolkit
/// resets a window's styles whenever one of its own flags changes, which a
/// window inside the desktop does not survive, so this comes first.
fn detach(w: &WebviewWindow) {
    if let Some(hwnd) = native_handle(w) {
        desktop::lift(hwnd);
    }
}

/// Put the widget on the layer the settings ask for. A widget pinned to the
/// desktop is lifted while "Show desktop" is active, or it would be covered;
/// in wallpaper mode it is part of the desktop and needs none of that.
fn apply_layer(app: &AppHandle) {
    let handle = app.clone();
    // On the main thread, so that our own window calls and the toolkit's
    // cannot interleave.
    let _ = app.run_on_main_thread(move || {
        let state = handle.state::<Shared>();
        let layer = state.settings.lock().unwrap().layer.clone();
        let Some(w) = handle.get_webview_window(WIDGET) else {
            return;
        };
        if layer == "wallpaper" && w.is_visible().unwrap_or(false) {
            if let Some(hwnd) = native_handle(&w) {
                if desktop::is_embedded(hwnd) {
                    desktop::refit(hwnd);
                    return;
                }
                let _ = w.set_always_on_top(false);
                let _ = w.set_always_on_bottom(false);
                if desktop::embed(hwnd) {
                    return;
                }
            }
            // No desktop to move into (yet): stay pinned under other windows.
            let _ = w.set_always_on_bottom(true);
            return;
        }
        detach(&w);
        let top = layer == "top" || state.desktop_shown.load(Ordering::Relaxed);
        let _ = w.set_always_on_bottom(!top);
        let _ = w.set_always_on_top(top);
    });
}

/// Keeps a desktop-pinned widget visible through "Show desktop" (Win+D), and
/// a wallpaper-mode widget inside the desktop.
fn watch_desktop(app: AppHandle, state: Shared) {
    std::thread::spawn(move || {
        let mut polls = 0u32;
        let mut widget = current_widget(&app);
        let mut vanished = false;
        loop {
            std::thread::sleep(DESKTOP_POLL);
            polls = polls.wrapping_add(1);
            let layer = state.settings.lock().unwrap().layer.clone();
            let shown = layer == "bottom" && desktop::is_desktop_shown();
            if state.desktop_shown.swap(shown, Ordering::Relaxed) != shown {
                apply_layer(&app);
            }
            // A widget inside the desktop vanishes without notice when
            // Explorer is killed or crashes. Its window cannot be replaced
            // then, as nothing knows that it is gone, so the app starts over.
            let gone = |handle: Option<desktop::Hwnd>| handle.is_some_and(|h| !desktop::exists(h));
            if widget.is_none() || gone(widget) {
                // The handle may just be out of date: the widget is reopened
                // when it gets closed, and there is none in between.
                widget = current_widget(&app);
                // Seen twice in a row, so that a window caught in the middle
                // of closing normally is not mistaken for a vanished one.
                if gone(widget) && vanished && !state.exiting.swap(true, Ordering::Relaxed) {
                    restart();
                }
                vanished = gone(widget);
            }
            if polls % EMBED_CHECK_POLLS != 0 {
                continue;
            }
            // The desktop may not be there yet right after sign-in, and the
            // widget is put out of it when Explorer closes.
            if layer == "wallpaper" && !state.peeking.load(Ordering::Relaxed) {
                apply_layer(&app);
            }
        }
    });
}

/// The widget's native window, if the widget is open.
fn current_widget(app: &AppHandle) -> Option<desktop::Hwnd> {
    app.get_webview_window(WIDGET).as_ref().and_then(native_handle)
}

/// Start a new instance and end this one, without the orderly shutdown: that
/// looks at every window, and fails on one that vanished.
fn restart() -> ! {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe)
            .arg(format!("{RESTART_FLAG}={}", std::process::id()))
            .spawn();
    }
    std::process::exit(0);
}

/// Open the widget again after its window was closed (Alt+F4, say).
fn reopen_widget(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(REOPEN_DELAY).await;
        let state = app.state::<Shared>().inner().clone();
        if state.exiting.load(Ordering::Relaxed) || app.get_webview_window(WIDGET).is_some() {
            return;
        }
        let Some(config) = app.config().app.windows.iter().find(|w| w.label == WIDGET) else {
            return;
        };
        let built = WebviewWindowBuilder::from_config(&app, config).and_then(|b| b.build());
        if built.is_ok() {
            apply_layer(&app);
            schedule_clamp(&app);
        }
    });
}

/// Move the widget so that all of it is inside its monitor's work area (the
/// screen minus the taskbar).
fn clamp_widget(app: &AppHandle) {
    let Some(w) = app.get_webview_window(WIDGET) else {
        return;
    };
    let monitor = w
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    let (Some(monitor), Ok(pos), Ok(size)) = (monitor, w.outer_position(), w.outer_size()) else {
        return;
    };
    let area = monitor.work_area();
    let (min_x, min_y) = (area.position.x, area.position.y);
    let max_x = (min_x + area.size.width as i32 - size.width as i32).max(min_x);
    let max_y = (min_y + area.size.height as i32 - size.height as i32).max(min_y);
    let (x, y) = (pos.x.clamp(min_x, max_x), pos.y.clamp(min_y, max_y));
    if (x, y) != (pos.x, pos.y) {
        // Inside the desktop, it is the widget's carrier that moves.
        if !native_handle(&w).is_some_and(|hwnd| desktop::move_embedded(hwnd, x, y)) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    }
}

/// Clamp the widget once it has stopped moving, and remember where it is.
/// Waiting for the mouse button avoids fighting the user in the middle of a
/// drag. The position is saved here because a tray app is rarely quit in an
/// orderly way, which is the only other moment it would be saved.
fn schedule_clamp(app: &AppHandle) {
    let state = app.state::<Shared>().inner().clone();
    let seq = state.clamp_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLAMP_DELAY).await;
        while desktop::is_dragging() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if state.clamp_seq.load(Ordering::Relaxed) == seq {
            clamp_widget(&app);
            // On the main thread: saving reads the window while holding a lock
            // that the main thread also takes on every move, so from here the
            // two would wait for each other forever.
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let _ = handle.save_window_state(StateFlags::POSITION);
            });
        }
    });
}

fn place_top_right(app: &AppHandle) {
    let Some(w) = app.get_webview_window(WIDGET) else {
        return;
    };
    if let (Ok(Some(monitor)), Ok(size)) = (w.primary_monitor(), w.outer_size()) {
        let margin = (24.0 * monitor.scale_factor()) as i32;
        let x = monitor.position().x + monitor.size().width as i32 - size.width as i32 - margin;
        let y = monitor.position().y + margin;
        let _ = w.set_position(PhysicalPosition::new(x, y));
    }
}

/// Bring the widget in front of other windows. It drops back to its normal
/// layer when it loses focus (see the window-event handler in `main`). In
/// wallpaper mode this is the way to click or move the widget.
fn peek_widget(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(WIDGET) {
            handle.state::<Shared>().peeking.store(true, Ordering::Relaxed);
            detach(&w);
            let _ = w.show();
            let _ = w.set_always_on_bottom(false);
            let _ = w.set_always_on_top(true);
            let _ = w.set_focus();
        }
    });
}

fn set_widget_visible(app: &AppHandle, visible: bool) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(w) = handle.get_webview_window(WIDGET) {
            if visible {
                let _ = w.show();
                apply_layer(&handle);
            } else {
                detach(&w);
                let _ = w.hide();
            }
        }
    });
}

fn toggle_widget(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WIDGET) {
        set_widget_visible(app, !w.is_visible().unwrap_or(false));
    }
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("index.html".into()))
        .title(format!("TokenTracker {}", t("설정", "Settings")))
        .additional_browser_args(web::BROWSER_ARGS)
        .inner_size(440.0, 640.0)
        .resizable(false)
        .maximizable(false)
        // A plain popup: the page draws its own header and close button.
        .decorations(false)
        .center()
        .initialization_script("window.__TT_VIEW__ = 'settings';")
        .build();
}

/// Commands are only for our own UI, never for the provider webviews.
fn guard(window: &WebviewWindow) -> Result<(), String> {
    match window.label() {
        WIDGET | SETTINGS => Ok(()),
        _ => Err("forbidden".into()),
    }
}

#[tauri::command]
fn get_snapshot(window: WebviewWindow, state: State<'_, Shared>) -> Result<Snapshot, String> {
    guard(&window)?;
    Ok(state.snapshot.lock().unwrap().clone())
}

#[tauri::command]
fn get_settings(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Shared>,
) -> Result<Settings, String> {
    guard(&window)?;
    let mut settings = state.settings.lock().unwrap().clone();
    // The system's autostart entry is the source of truth.
    settings.autostart = app.autolaunch().is_enabled().unwrap_or(settings.autostart);
    Ok(settings)
}

#[tauri::command]
fn set_settings(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Shared>,
    settings: Settings,
) -> Result<(), String> {
    guard(&window)?;
    let new = settings.sanitized();
    let old = std::mem::replace(&mut *state.settings.lock().unwrap(), new.clone());

    if new.autostart != old.autostart {
        let auto = app.autolaunch();
        let result = if new.autostart {
            auto.enable()
        } else {
            auto.disable()
        };
        result.map_err(|e| e.to_string())?;
    }
    if new.layer != old.layer {
        apply_layer(&app);
    }
    let language_changed = new.language != old.language;
    if language_changed {
        i18n::set_language(&new.language);
        if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY), tray_menu(&app)) {
            let _ = tray.set_menu(Some(menu));
        }
        if let Some(w) = app.get_webview_window(SETTINGS) {
            let _ = w.set_title(&format!("TokenTracker {}", t("설정", "Settings")));
        }
    }
    for account in old.accounts.iter().filter(|a| new.account(&a.id).is_none()) {
        remove_account(&app, account);
    }
    if new.accounts != old.accounts {
        sync_snapshot(&app, &state, &new.accounts);
    }
    settings::save(&app, &new);
    let _ = app.emit("settings", &new);

    let newly_shown = new
        .accounts
        .iter()
        .any(|a| a.shown && !old.account(&a.id).is_some_and(|o| o.shown));
    // A new language needs new labels and messages, which come with a refresh.
    if newly_shown
        || language_changed
        || new.refresh_minutes != old.refresh_minutes
        || new.antigravity_launch != old.antigravity_launch
    {
        state.wake.notify_one();
    }
    Ok(())
}

#[tauri::command]
fn refresh_now(window: WebviewWindow, state: State<'_, Shared>) -> Result<(), String> {
    guard(&window)?;
    state.wake.notify_one();
    Ok(())
}

#[tauri::command]
fn hide_widget(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    guard(&window)?;
    set_widget_visible(&app, false);
    Ok(())
}

// Window-creating commands are async: building a webview from a synchronous
// command deadlocks on Windows.

#[tauri::command]
async fn open_settings(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    guard(&window)?;
    show_settings(&app);
    Ok(())
}

fn find_account(state: &AppState, id: &str) -> Result<Account, String> {
    state
        .settings
        .lock()
        .unwrap()
        .account(id)
        .cloned()
        .ok_or_else(|| "unknown account".to_string())
}

/// Open the sign-in window of an account that is read through a webview.
#[tauri::command]
async fn provider_login(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Shared>,
    account: String,
) -> Result<(), String> {
    guard(&window)?;
    web::open_login(&app, &find_account(&state, &account)?);
    state.wake.notify_one();
    Ok(())
}

/// Forget the session this app holds for an account.
#[tauri::command]
async fn provider_disconnect(
    window: WebviewWindow,
    app: AppHandle,
    state: State<'_, Shared>,
    account: String,
) -> Result<(), String> {
    guard(&window)?;
    web::disconnect(&app, &find_account(&state, &account)?);
    state.wake.notify_one();
    Ok(())
}

/// The tray menu, in the current language.
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, ko: &'static str, en: &'static str| {
        MenuItem::with_id(app, id, t(ko, en), true, None::<&str>)
    };
    let peek = item("peek", "위젯 앞으로 가져오기", "Bring widget to front")?;
    let toggle = item("toggle", "위젯 표시/숨김", "Show/hide widget")?;
    let refresh = item("refresh", "새로고침", "Refresh")?;
    let settings = item("settings", "설정", "Settings")?;
    let quit = item("quit", "종료", "Quit")?;
    Menu::with_items(app, &[&peek, &toggle, &refresh, &settings, &quit])
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app)?;
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("TokenTracker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "peek" => peek_widget(app),
            "toggle" => toggle_widget(app),
            "refresh" => app.state::<Shared>().wake.notify_one(),
            "settings" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { show_settings(&app) });
            }
            "quit" => {
                app.state::<Shared>().exiting.store(true, Ordering::Relaxed);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // The widget usually sits behind other windows, so a click
                // brings it forward rather than hiding it.
                peek_widget(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    // Started by an instance that is restarting: wait until it is gone, or it
    // would be taken for a running instance and this one would step aside.
    let old_instance = std::env::args()
        .find_map(|a| a.strip_prefix(RESTART_FLAG)?.strip_prefix('=')?.parse::<u32>().ok());
    if let Some(id) = old_instance {
        desktop::wait_for_exit(id, RESTART_WAIT_MS);
    }

    // WebKitGTK's DMA-BUF renderer leaves windows blank or crashes on many
    // NVIDIA and Wayland setups; the fallback draws the same for a widget.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let state: Shared = Arc::new(AppState {
        snapshot: Mutex::new(Snapshot::default()),
        settings: Mutex::new(Settings::default()),
        alert_levels: Mutex::new(HashMap::new()),
        pending: Mutex::new(HashMap::new()),
        wake: Notify::new(),
        desktop_shown: AtomicBool::new(false),
        peeking: AtomicBool::new(false),
        exiting: AtomicBool::new(false),
        clamp_seq: AtomicU64::new(0),
    });

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Launching the app again brings the running widget forward, or
            // opens the settings when started with `--settings`.
            if args.iter().any(|a| a == "--settings") {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { show_settings(&app) });
            } else {
                peek_widget(app);
            }
        }))
        .on_window_event(|window, event| {
            if window.label() != WIDGET {
                return;
            }
            match event {
                WindowEvent::Focused(false) => {
                    let app = window.app_handle();
                    app.state::<Shared>().peeking.store(false, Ordering::Relaxed);
                    apply_layer(app);
                }
                WindowEvent::Destroyed => reopen_widget(window.app_handle()),
                // Dragged, resized by a view change, or the display changed.
                WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. } => {
                    let app = window.app_handle();
                    if let Some(hwnd) = app.get_webview_window(WIDGET).as_ref().and_then(native_handle)
                    {
                        desktop::refit(hwnd);
                    }
                    schedule_clamp(app);
                }
                _ => {}
            }
        })
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::POSITION)
                // Only the widget's position is worth remembering.
                .with_filter(|label| label == WIDGET)
                .build(),
        )
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_settings,
            set_settings,
            refresh_now,
            hide_widget,
            open_settings,
            provider_login,
            provider_disconnect,
        ])
        .setup(move |app| {
            // A tray app: no Dock icon, and no menu bar of its own.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let handle = app.handle().clone();
            let stored = settings::load(&handle);
            let first_run = stored.is_none();
            let settings = stored.unwrap_or_default();
            i18n::set_language(&settings.language);
            *state.settings.lock().unwrap() = settings.clone();
            sync_snapshot(&handle, &state, &settings.accounts);
            settings::remove_orphan_profiles(&handle, &settings);

            apply_layer(&handle);
            if first_run {
                place_top_right(&handle);
                settings::save(&handle, &settings);
                // Show the user where the widget landed the first time.
                peek_widget(&handle);
            }
            // A saved position may be off screen after a display change.
            schedule_clamp(&handle);
            build_tray(&handle)?;
            watch_desktop(handle.clone(), state.clone());
            tauri::async_runtime::spawn(poll_loop(handle, state.clone()));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building TokenTracker");

    // Closing windows must not quit a tray app; only the tray's "종료" does.
    app.run(|_app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
