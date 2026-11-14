//! Antigravity plan usage: its Gemini models and all the others ("Others").
//!
//! While Antigravity runs, its language server listens on a local port and
//! writes how to reach it to `~/.gemini/antigravity/daemon/*.json`. We ask
//! that server for the signed-in user's status, which lists the quota left for
//! every model. The server's token is only ever sent to 127.0.0.1.
//!
//! While Antigravity is closed, its quota cannot be used up, so we show the
//! last answer we had. Only if the user turns on `antigravity_launch` do we
//! start its language server ourselves for a few seconds, the way Antigravity
//! starts it, ask it the same question and end it. It signs in from
//! Antigravity's own saved login; we never see that login, only the quota it
//! reports.

use crate::{
    i18n::t,
    model::{now_secs, LimitWindow, ProviderStatus},
    settings::Account,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant, SystemTime},
};

/// Name in messages.
pub const GROUP: &str = "Antigravity";

/// What the local Antigravity had to say.
#[derive(Clone)]
pub enum Quota {
    Ok(Vec<LimitWindow>),
    /// Antigravity is not installed, or its server could not be started.
    Offline,
    /// Antigravity is not running, and we were not to start its server.
    Closed,
    NeedsLogin,
    Error(String),
}

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
const STATUS_BODY: &str =
    r#"{"metadata":{"ideName":"antigravity","extensionName":"antigravity","locale":"en"}}"#;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            // The server is on this PC; a system proxy must not see the token.
            .no_proxy()
            .build()
            .expect("failed to build HTTP client")
    })
}

/// Where a language server said it can be reached.
struct Endpoint {
    port: u16,
    token: String,
    written: SystemTime,
}

fn daemon_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".gemini").join("antigravity").join("daemon"))
}

/// Every server that left a note, newest first. Servers that have exited leave
/// their notes behind, so some of these no longer answer.
fn endpoints() -> Vec<Endpoint> {
    let Some(entries) = daemon_dir().and_then(|d| fs::read_dir(d).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<Endpoint> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let json: Value = serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok()?;
            Some(Endpoint {
                port: u16::try_from(json.get("httpPort")?.as_u64()?).ok()?,
                token: json.get("csrfToken")?.as_str()?.to_string(),
                written: e.metadata().and_then(|m| m.modified()).ok()?,
            })
        })
        .filter(|e| e.port != 0 && !e.token.is_empty())
        .collect();
    found.sort_by(|a, b| b.written.cmp(&a.written));
    found
}

/// Epoch seconds of a UTC timestamp such as `2026-11-18T10:21:23Z`.
fn parse_utc(s: &str) -> Option<i64> {
    if !s.is_ascii() || !s.ends_with('Z') {
        return None;
    }
    let num = |from: usize, to: usize| s.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days since 1970-01-01, counting years from March so the leap day is last.
    let year = if month <= 2 { year - 1 } else { year };
    let month = if month <= 2 { month + 9 } else { month - 3 };
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = year.div_euclid(400) * 146_097 + day_of_era - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Quota of one model family, e.g. every reasoning level of "Gemini Flash".
#[derive(Debug, PartialEq)]
struct Family {
    /// First word of the model name: "Gemini", "Claude", …
    vendor: String,
    name: String,
    used_percent: f64,
    resets_at: Option<i64>,
}

/// "Gemini 3.8 Flash (High)" → ("Gemini", "Gemini Flash"): the variant in
/// parentheses and the version numbers do not matter for the quota.
fn family_of(label: &str) -> Option<(String, String)> {
    let mut depth = 0u32;
    let plain: String = label
        .chars()
        .filter(|c| {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => return depth == 0,
            }
            false
        })
        .collect();
    let mut words = plain.split_whitespace();
    let vendor = words.next()?.to_string();
    let rest: Vec<&str> = words
        .filter(|w| !w.chars().any(|c| c.is_ascii_digit()))
        .collect();
    let name = std::iter::once(vendor.as_str())
        .chain(rest)
        .collect::<Vec<_>>()
        .join(" ");
    Some((vendor, name))
}

fn families(models: &[Value]) -> Vec<Family> {
    let mut out: Vec<Family> = Vec::new();
    for model in models {
        if model.get("disabled").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let Some(quota) = model.get("quotaInfo") else { continue };
        let remaining = quota.get("remainingFraction").and_then(Value::as_f64);
        let resets_at = quota
            .get("resetTime")
            .and_then(Value::as_str)
            .and_then(parse_utc);
        if remaining.is_none() && resets_at.is_none() {
            continue;
        }
        let Some((vendor, name)) = model
            .get("label")
            .and_then(Value::as_str)
            .and_then(family_of)
        else {
            continue;
        };
        // A fraction of zero is left out of the response entirely.
        let used_percent = (1.0 - remaining.unwrap_or(0.0).clamp(0.0, 1.0)) * 100.0;
        match out.iter_mut().find(|f| f.name == name) {
            // Show the most used variant of a family.
            Some(family) if used_percent > family.used_percent => {
                family.used_percent = used_percent;
                family.resets_at = resets_at;
            }
            Some(_) => {}
            None => out.push(Family {
                vendor,
                name,
                used_percent,
                resets_at,
            }),
        }
    }
    out
}

/// Two rows: Google's own models ("Gemini") and everything else ("Others":
/// Claude, GPT-OSS, …, which share one quota). Each row shows the most used
/// family of its set.
fn windows(families: Vec<Family>) -> Vec<LimitWindow> {
    fn most<'a>(set: impl Iterator<Item = &'a Family>) -> Option<&'a Family> {
        set.max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))
    }
    let is_gemini = |f: &&Family| f.vendor.eq_ignore_ascii_case("gemini");
    let gemini = most(families.iter().filter(is_gemini));
    let others = most(families.iter().filter(|f| !is_gemini(f)));
    [(gemini, "gemini", "Gemini"), (others, "others", "Others")]
        .into_iter()
        .filter_map(|(family, key, label)| {
            let family = family?;
            Some(LimitWindow {
                key: format!("agy:{key}"),
                label: label.into(),
                used_percent: family.used_percent,
                resets_at: family.resets_at,
                // Weekly on the free plan, a few hours on paid ones; not reported.
                period_seconds: None,
                group: None,
            })
        })
        .collect()
}

fn parse(json: &Value) -> Quota {
    let Some(user) = json.get("userStatus") else {
        return Quota::NeedsLogin;
    };
    let models = user
        .pointer("/cascadeModelConfigData/clientModelConfigs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let windows = windows(families(models));
    if windows.is_empty() {
        return Quota::Error(crate::i18n::no_limits().into());
    }
    Quota::Ok(windows)
}

/// Families from the full model list (`GetAvailableModels`). Unlike the user
/// status, it has the Gemini models even on a server we started ourselves.
/// Google's models count as "Gemini", all others as one other vendor.
fn available_families(json: &Value) -> Vec<Family> {
    let Some(models) = json.pointer("/response/models").and_then(Value::as_object) else {
        return Vec::new();
    };
    let flag = |m: &Value, key: &str| m.get(key).and_then(Value::as_bool).unwrap_or(false);
    models
        .iter()
        .filter(|(_, m)| !flag(m, "isInternal") && !flag(m, "disabled"))
        .filter_map(|(name, m)| {
            let quota = m.get("quotaInfo")?;
            // Models without a reset time are helpers (autocomplete) that
            // have no quota of their own.
            let resets_at = quota.get("resetTime").and_then(Value::as_str).and_then(parse_utc)?;
            // A fraction of zero is left out of the response entirely.
            let remaining = quota.get("remainingFraction").and_then(Value::as_f64).unwrap_or(0.0);
            let google = m.get("modelProvider").and_then(Value::as_str) == Some("MODEL_PROVIDER_GOOGLE");
            Some(Family {
                vendor: if google { "Gemini" } else { "Other" }.into(),
                name: name.clone(),
                used_percent: (1.0 - remaining.clamp(0.0, 1.0)) * 100.0,
                resets_at: Some(resets_at),
            })
        })
        .collect()
}

/// Posts to one of the server's methods. `None` when the server does not
/// answer at all; `Err` with the status code when it refuses.
async fn call(port: u16, token: &str, method: &str) -> Option<Result<Value, u16>> {
    let resp = client()
        .post(format!("http://127.0.0.1:{port}/{SERVICE}/{method}"))
        .header("content-type", "application/json")
        .header("connect-protocol-version", "1")
        .header("x-codeium-csrf-token", token)
        .body(STATUS_BODY)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return Some(Err(resp.status().as_u16()));
    }
    Some(resp.json::<Value>().await.map_err(|_| 0))
}

/// What one language server said, or `None` when it did not answer at all.
async fn ask(port: u16, token: &str) -> Option<Quota> {
    let status = match call(port, token, "GetUserStatus").await? {
        Ok(json) => json,
        Err(0) => return Some(Quota::Error(crate::i18n::unreadable().into())),
        Err(code) => return Some(Quota::Error(crate::i18n::bad_reply(GROUP, &code.to_string()))),
    };
    let quota = parse(&status);
    if matches!(quota, Quota::NeedsLogin) {
        return Some(quota);
    }
    // The full model list, where it is to be had, also covers Gemini.
    if let Some(Ok(all)) = call(port, token, "GetAvailableModels").await {
        let rows = windows(available_families(&all));
        if !rows.is_empty() {
            return Some(Quota::Ok(rows));
        }
    }
    Some(quota)
}

/// The account's status for the widget.
/// The last quota any server reported, and when (epoch seconds).
fn last_seen() -> &'static Mutex<Option<(i64, Vec<LimitWindow>)>> {
    static LAST: Mutex<Option<(i64, Vec<LimitWindow>)>> = Mutex::new(None);
    &LAST
}

/// Windows as they stand at `now`: one whose reset time has passed starts
/// over empty, with its next reset one period later when the period is known.
fn aged(windows: Vec<LimitWindow>, now: i64) -> Vec<LimitWindow> {
    windows
        .into_iter()
        .map(|mut w| {
            if let Some(mut reset) = w.resets_at.filter(|r| *r <= now) {
                w.used_percent = 0.0;
                w.resets_at = match w.period_seconds.filter(|p| *p > 0) {
                    Some(period) => {
                        reset += ((now - reset) / period + 1) * period;
                        Some(reset)
                    }
                    None => None,
                };
            }
            w
        })
        .collect()
}

/// `launch`: the user allowed starting Antigravity's server while it is closed.
pub async fn status(account: &Account, fresh: bool, launch: bool) -> ProviderStatus {
    let status = ProviderStatus::new(account);
    match fetch(fresh, launch).await {
        Quota::Ok(windows) => {
            *last_seen().lock().unwrap() = Some((now_secs(), windows.clone()));
            status.ok(None, windows)
        }
        Quota::Closed => match last_seen().lock().unwrap().clone() {
            Some((at, windows)) => {
                let mut status = status.ok(None, aged(windows, now_secs()));
                status.updated_at = Some(at);
                status.note = Some(
                    t(
                        "Antigravity가 꺼져 있어 마지막으로 확인한 값을 보여 줍니다.",
                        "Antigravity is closed; showing the last values seen.",
                    )
                    .into(),
                );
                status
            }
            None => status.needs_login(t(
                "Antigravity를 실행하면 사용량이 표시됩니다.",
                "Open Antigravity to see its usage.",
            )),
        },
        Quota::NeedsLogin => status.needs_login(t(
            "Antigravity 앱에서 Google 계정으로 로그인해 주세요.",
            "Sign in to the Antigravity app with your Google account.",
        )),
        Quota::Offline => status.needs_login(t(
            "이 PC에서 Antigravity를 찾지 못했습니다.",
            "Antigravity was not found on this PC.",
        )),
        Quota::Error(message) => status.error(message),
    }
}

/// `fresh`: the user asked for a refresh, so a recent answer of a server we
/// started is not good enough.
async fn fetch(fresh: bool, launch: bool) -> Quota {
    let mut refused: Option<Quota> = None;
    for endpoint in endpoints() {
        match ask(endpoint.port, &endpoint.token).await {
            // No answer means the note is from a server that has exited.
            None => continue,
            Some(quota @ Quota::Error(_)) => refused = Some(quota),
            Some(quota) => return quota,
        }
    }
    if let Some(quota) = refused {
        return quota;
    }
    if !launch {
        return Quota::Closed;
    }
    fetch_standalone(fresh).await
}

// ---- Antigravity closed: start its language server for a moment ----------

/// Starting the server takes a moment and some memory, so while Antigravity
/// stays closed the automatic refresh does this at most this often; in
/// between, the last answer is reused. The refresh button always asks anew.
const STANDALONE_EVERY: Duration = Duration::from_secs(5 * 60);
/// How long the server gets to start and to sign in.
const STANDALONE_START: Duration = Duration::from_secs(20);

/// The last answer of a server we started, and when it came.
fn standalone_cache() -> &'static Mutex<Option<(Instant, Quota)>> {
    static CACHE: Mutex<Option<(Instant, Quota)>> = Mutex::new(None);
    &CACHE
}

/// Program and arguments with which Antigravity last started its language
/// server, from its log, minus the parts that are per run.
fn launch_line() -> Option<(PathBuf, Vec<String>)> {
    let log = dirs::config_dir()?.join("Antigravity").join("logs").join("main.log");
    let text = fs::read_to_string(log).ok()?;
    let (exe, args) = text.lines().rev().find_map(parse_spawn_line)?;
    exe.is_file().then_some((exe, args))
}

/// "… Spawning: C:\…\language_server.exe --standalone …" → program and
/// arguments, without the per-run ones. On macOS and Linux the program has no
/// ".exe"; it ends where the first option starts.
fn parse_spawn_line(line: &str) -> Option<(PathBuf, Vec<String>)> {
    let (_, line) = line.split_once("Spawning: ")?;
    let (exe, rest) = match line.split_once(".exe ") {
        Some((exe, rest)) => (format!("{exe}.exe"), rest),
        None => {
            let at = line.find(" --")?;
            (line[..at].trim().to_string(), &line[at + 1..])
        }
    };
    let exe = PathBuf::from(exe);
    let mut args = Vec::new();
    let mut words = rest.split_whitespace();
    while let Some(word) = words.next() {
        match word {
            // Ours are chosen below; sidecars are helpers of the app's UI.
            w if w.starts_with("--csrf_token") || w == "--enable_sidecars" => {}
            "--https_server_port" => {
                words.next();
            }
            w => args.push(w.to_string()),
        }
    }
    Some((exe, args))
}

/// The usual place, for when the log has nothing to say. Only known for the
/// Windows installer; elsewhere the log is the only source.
#[cfg(not(windows))]
fn default_launch() -> Option<(PathBuf, Vec<String>)> {
    None
}

/// The usual place, for when the log has nothing to say.
#[cfg(windows)]
fn default_launch() -> Option<(PathBuf, Vec<String>)> {
    let exe = dirs::data_local_dir()?
        .join("Programs")
        .join("antigravity")
        .join("resources")
        .join("bin")
        .join("language_server.exe");
    let args = [
        "--standalone",
        "--override_ide_name",
        "antigravity",
        "--subclient_type",
        "hub",
        "--override_user_agent_name",
        "antigravity",
        "--app_data_dir",
        "antigravity",
        "--api_server_url",
        "https://generativelanguage.googleapis.com",
        "--cloud_code_endpoint",
        "https://daily-cloudcode-pa.googleapis.com",
    ];
    exe.is_file()
        .then(|| (exe, args.iter().map(|a| a.to_string()).collect()))
}

/// Ends the server however `fetch_standalone` is left, and removes the note
/// it wrote about itself.
struct Started(Child);

impl Drop for Started {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        if let Some(dir) = daemon_dir() {
            remove_notes_of(&dir, self.0.id());
        }
    }
}

/// Remove every note written by the process `pid`.
fn remove_notes_of(dir: &Path, pid: u32) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let mine = fs::read_to_string(entry.path())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|j| j.get("pid")?.as_u64())
            == Some(u64::from(pid));
        if mine {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The port the server `pid` says it listens on, once it has said so.
fn port_of(pid: u32) -> Option<u16> {
    fs::read_dir(daemon_dir()?)
        .ok()?
        .flatten()
        .find_map(|e| {
            let json: Value = serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok()?;
            (json.get("pid")?.as_u64()? == u64::from(pid))
                .then(|| u16::try_from(json.get("httpPort")?.as_u64()?).ok())
                .flatten()
        })
        .filter(|p| *p != 0)
}

/// "… listening on random port at 58157 for HTTP" → 58157.
fn http_port_in(line: &str) -> Option<u16> {
    let rest = line.trim_end().strip_suffix(" for HTTP")?;
    let (_, port) = rest.rsplit_once(" at ")?;
    port.parse().ok().filter(|p| *p != 0)
}

/// A fresh secret for the one server we start. `RandomState` is seeded from
/// the system's random source on every call.
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..4)
        .map(|_| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u8(0);
            format!("{:016x}", h.finish())
        })
        .collect()
}

async fn fetch_standalone(fresh: bool) -> Quota {
    if let Some((at, quota)) = standalone_cache().lock().unwrap().as_ref() {
        if !fresh && at.elapsed() < STANDALONE_EVERY {
            return quota.clone();
        }
    }
    let quota = run_standalone().await;
    *standalone_cache().lock().unwrap() = Some((Instant::now(), quota.clone()));
    quota
}

async fn run_standalone() -> Quota {
    let Some((exe, args)) = launch_line().or_else(default_launch) else {
        return Quota::Offline; // not installed
    };

    let token = random_token();
    let mut command = Command::new(&exe);
    command
        .args(&args)
        .arg("--https_server_port")
        .arg("0")
        .arg(format!("--csrf_token={token}"))
        // Like a language server, it quits once its input closes; ours has
        // none of its own, so it gets a pipe that stays open until we end it.
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        // Its log tells the port it picked.
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let Ok(mut child) = command.spawn() else {
        return Quota::Offline;
    };
    let heard = Arc::new(AtomicU16::new(0));
    if let Some(log) = child.stderr.take() {
        let heard = heard.clone();
        // Read to the end, so the server never waits on a full pipe.
        std::thread::spawn(move || {
            for line in BufReader::new(log).lines().map_while(Result::ok) {
                if let Some(port) = http_port_in(&line) {
                    heard.store(port, Ordering::Relaxed);
                }
            }
        });
    }
    let started = Started(child);
    let pid = started.0.id();

    let deadline = Instant::now() + STANDALONE_START;
    let mut last = Quota::Offline;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let port = match heard.load(Ordering::Relaxed) {
            0 => port_of(pid),
            p => Some(p),
        };
        let Some(port) = port else { continue };
        match ask(port, &token).await {
            Some(quota @ Quota::Ok(_)) => return quota,
            // Signing in may take a moment after the server is up.
            Some(other) => last = other,
            None => {}
        }
    }
    drop(started);
    last
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn window(used: f64, resets_at: Option<i64>, period: Option<i64>) -> LimitWindow {
        LimitWindow {
            key: "k".into(),
            label: "l".into(),
            used_percent: used,
            resets_at,
            period_seconds: period,
            group: None,
        }
    }

    #[test]
    fn last_values_age_past_their_reset() {
        let now = 1_000_000;
        let out = aged(
            vec![
                window(40.0, Some(now + 60), Some(3_600)),
                window(70.0, Some(now - 10), Some(3_600)),
                window(90.0, Some(now - 7_300), Some(3_600)),
                window(55.0, Some(now - 1), None),
            ],
            now,
        );
        assert_eq!((out[0].used_percent, out[0].resets_at), (40.0, Some(now + 60)));
        assert_eq!((out[1].used_percent, out[1].resets_at), (0.0, Some(now + 3_590)));
        assert_eq!((out[2].used_percent, out[2].resets_at), (0.0, Some(now + 3_500)));
        assert_eq!((out[3].used_percent, out[3].resets_at), (0.0, None));
    }

    #[test]
    fn parses_utc_timestamps() {
        assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_utc("2026-11-18T10:21:23Z"), Some(1_794_997_283));
        assert_eq!(parse_utc("2024-02-29T23:59:59.250Z"), Some(1_709_251_199));
        assert_eq!(parse_utc("2026-11-18 10:21"), None);
    }

    /// Starts the installed Antigravity's server for real. Run by hand:
    /// `cargo test standalone -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn standalone_against_installed_antigravity() {
        let started = Instant::now();
        let quota = tauri::async_runtime::block_on(fetch_standalone(true));
        let seen = match &quota {
            Quota::Ok(w) => format!("ok, {} rows: {:?}", w.len(), w.iter().map(|w| &w.label).collect::<Vec<_>>()),
            Quota::Offline => "offline".into(),
            Quota::Closed => "closed".into(),
            Quota::NeedsLogin => "needs login".into(),
            Quota::Error(e) => format!("error: {e}"),
        };
        println!("{seen} after {:?}", started.elapsed());
        assert!(matches!(quota, Quota::Ok(_)), "{seen}");
    }

    #[test]
    fn reads_the_launch_line() {
        let line = r"[info] Spawning: C:\Apps\antigravity\resources\bin\language_server.exe --standalone --app_data_dir antigravity --https_server_port 0 --csrf_token=abc --enable_sidecars --api_server_url https://x";
        let (exe, args) = parse_spawn_line(line).unwrap();
        assert_eq!(exe, PathBuf::from(r"C:\Apps\antigravity\resources\bin\language_server.exe"));
        assert_eq!(
            args,
            ["--standalone", "--app_data_dir", "antigravity", "--api_server_url", "https://x"]
        );
        assert!(parse_spawn_line("[info] Starting window").is_none());
    }

    #[test]
    fn reads_a_unix_launch_line() {
        let line = "[info] Spawning: /Applications/Antigravity.app/Contents/Resources/app/bin/language_server_macos_arm --standalone --https_server_port 0 --csrf_token=abc --app_data_dir antigravity";
        let (exe, args) = parse_spawn_line(line).unwrap();
        assert_eq!(
            exe,
            PathBuf::from("/Applications/Antigravity.app/Contents/Resources/app/bin/language_server_macos_arm")
        );
        assert_eq!(args, ["--standalone", "--app_data_dir", "antigravity"]);
    }

    #[test]
    fn reads_the_full_model_list() {
        let reset = "2026-11-21T01:37:01Z";
        let all = json!({ "response": { "models": {
            "gemini-3.8-flash-tiered": { "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": { "remainingFraction": 0.6, "resetTime": reset } },
            "gemini-3-flash": { "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": { "remainingFraction": 1, "resetTime": reset } },
            "claude-opus-4-6-thinking": { "modelProvider": "MODEL_PROVIDER_ANTHROPIC",
                "quotaInfo": { "resetTime": reset } },
            "gpt-oss-120b-medium": { "modelProvider": "MODEL_PROVIDER_OPENAI",
                "quotaInfo": { "remainingFraction": 0.8, "resetTime": reset } },
            "tab_flash_lite_preview": { "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": { "remainingFraction": 0 } },
            "chat_20706": { "isInternal": true, "modelProvider": "MODEL_PROVIDER_GOOGLE",
                "quotaInfo": { "remainingFraction": 0, "resetTime": reset } },
            "gemini-4-argon-disabled": { "disabled": true, "modelProvider": "MODEL_PROVIDER_GOOGLE" }
        } } });
        let rows = windows(available_families(&all));
        let seen: Vec<(&str, i64)> = rows
            .iter()
            .map(|w| (w.label.as_str(), w.used_percent.round() as i64))
            .collect();
        // Opus has no fraction left: it is used up.
        assert_eq!(seen, [("Gemini", 40), ("Others", 100)]);
    }

    #[test]
    fn hears_the_port() {
        let log = "I1114 08:55:49.218606  1 server.go:1107] Language server listening on random port at 58157 for HTTP";
        assert_eq!(http_port_in(log), Some(58157));
        assert_eq!(http_port_in("… listening on random port at 58156 for HTTPS (gRPC)"), None);
    }

    #[test]
    fn names_families() {
        let name = |label| family_of(label).map(|(_, name)| name);
        assert_eq!(name("Gemini 3.8 Flash (High)").as_deref(), Some("Gemini Flash"));
        assert_eq!(name("Claude Opus 4.6 (Thinking)").as_deref(), Some("Claude Opus"));
        assert_eq!(name("GPT-OSS 120B (Medium)").as_deref(), Some("GPT-OSS"));
        assert_eq!(name("  "), None);
    }

    fn model(label: &str, remaining: Option<f64>, reset: &str) -> Value {
        let mut quota = json!({ "resetTime": reset });
        if let Some(r) = remaining {
            quota["remainingFraction"] = json!(r);
        }
        json!({ "label": label, "quotaInfo": quota })
    }

    #[test]
    fn groups_models_into_rows() {
        let reset = "2026-11-18T10:21:23Z";
        let rows = windows(families(&[
            model("Gemini 3.8 Flash (High)", Some(1.0), reset),
            model("Gemini 3.8 Flash (Low)", Some(0.75), reset),
            model("Claude Sonnet 4.6 (Thinking)", Some(0.5), reset),
            model("Claude Opus 4.6 (Thinking)", None, reset),
            json!({ "label": "Gemini 4 Carbon", "disabled": true, "quotaInfo": {} }),
        ]));
        let seen: Vec<(&str, i64)> = rows
            .iter()
            .map(|w| (w.label.as_str(), w.used_percent.round() as i64))
            .collect();
        assert_eq!(seen, [("Gemini", 25), ("Others", 100)]);
        assert_eq!(rows[0].key, "agy:gemini");
        assert_eq!(rows[1].key, "agy:others");
        assert_eq!(rows[0].resets_at, Some(1_794_997_283));
    }
}
