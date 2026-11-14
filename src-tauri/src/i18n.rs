//! Korean or English text for what the Rust side shows: tray menu, window
//! titles, notifications, limit labels and status messages.
//!
//! The language is one global switch, set from the settings. Call sites keep
//! both texts side by side: `t("주간", "Weekly")`.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_language(language: &str) {
    ENGLISH.store(language == "en", Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// The text for the current language.
pub fn t(ko: &'static str, en: &'static str) -> &'static str {
    if is_english() {
        en
    } else {
        ko
    }
}

// Messages that several providers share.

pub fn unreadable() -> &'static str {
    t("응답을 해석하지 못했습니다.", "Could not read the response.")
}

pub fn offline() -> &'static str {
    t("네트워크에 연결할 수 없습니다.", "Cannot reach the network.")
}

pub fn no_limits() -> &'static str {
    t("한도 정보가 응답에 없습니다.", "The response has no limit information.")
}

pub fn timed_out() -> &'static str {
    t("응답 시간 초과", "Timed out")
}

/// "Claude 응답 오류 (HTTP 500)" / "Claude error (HTTP 500)".
pub fn bad_reply(service: &str, detail: &str) -> String {
    if is_english() {
        format!("{service} error ({detail})")
    } else {
        format!("{service} 응답 오류 ({detail})")
    }
}

/// Label of a limit window of the given length.
pub fn window_label(seconds: i64) -> String {
    let en = is_english();
    match seconds {
        18_000 => t("5시간", "5-hour").into(),
        604_800 => t("주간", "Weekly").into(),
        s if s > 0 && s % 86_400 == 0 => {
            format!("{}{}", s / 86_400, if en { "-day" } else { "일" })
        }
        s if s > 0 && s % 3_600 == 0 => {
            format!("{}{}", s / 3_600, if en { "-hour" } else { "시간" })
        }
        s => format!("{}{}", s / 60, if en { "-min" } else { "분" }),
    }
}

/// "ko" when the system is set to Korean, otherwise "en".
pub fn system_language() -> &'static str {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        const LANG_KOREAN: u16 = 0x12;
        if unsafe { GetUserDefaultUILanguage() } & 0x3ff == LANG_KOREAN {
            return "ko";
        }
        "en"
    }
    #[cfg(not(windows))]
    {
        // "ko-KR", "ko_KR.UTF-8", …
        match sys_locale::get_locale() {
            Some(locale) if locale.to_ascii_lowercase().starts_with("ko") => "ko",
            _ => "en",
        }
    }
}
