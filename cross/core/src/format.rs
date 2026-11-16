//! 시간 표기와 상태 문구 (한국어·영어 지원)

use chrono::{FixedOffset, Local, TimeZone, Timelike};

use crate::i18n::{self, Lang};
use crate::scheduler::EngineState;
use crate::Timestamp;

/// 잠금 시간 표시: 0 → "잠그지 않음"/"Never", 90 → "1분 30초"/"1 min 30 sec"
pub fn timeout(seconds: i64) -> String {
    timeout_in(i18n::current(), seconds)
}

pub fn timeout_in(lang: Lang, seconds: i64) -> String {
    if seconds <= 0 {
        return lang.pick("잠그지 않음", "Never").into();
    }
    duration_in(lang, seconds)
}

/// 기간 표시: 45 → "45초"/"45 sec", 120 → "2분"/"2 min", 5400 → "1시간 30분"/"1 hr 30 min"
pub fn duration(seconds: i64) -> String {
    duration_in(i18n::current(), seconds)
}

pub fn duration_in(lang: Lang, seconds: i64) -> String {
    let s = seconds.max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    let mut parts = Vec::new();
    match lang {
        Lang::Ko => {
            if h > 0 {
                parts.push(format!("{h}시간"));
            }
            if m > 0 {
                parts.push(format!("{m}분"));
            }
            if sec > 0 || parts.is_empty() {
                parts.push(format!("{sec}초"));
            }
        }
        Lang::En => {
            if h > 0 {
                parts.push(format!("{h} hr"));
            }
            if m > 0 {
                parts.push(format!("{m} min"));
            }
            if sec > 0 || parts.is_empty() {
                parts.push(format!("{sec} sec"));
            }
        }
    }
    parts.join(" ")
}

/// 남은 시간 표시 (올림): 59.2 → "1:00", 3725 → "1:02:05"
pub fn countdown(remaining: f64) -> String {
    let total = remaining.ceil().max(0.0) as i64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// 메뉴 막대 아이콘 옆 남은 시간.
/// 초 단위: "4:59"처럼 매 초 바뀐다.
/// 분 단위: 1분 넘게 남은 동안은 "5분"/"5m"(올림)처럼 1분에 한 번만 바뀌고, 마지막 1분은 "59초"/"59s"처럼 초 단위로 센다.
pub fn menu_bar_countdown(remaining: f64, in_minutes: bool) -> String {
    menu_bar_countdown_in(i18n::current(), remaining, in_minutes)
}

pub fn menu_bar_countdown_in(lang: Lang, remaining: f64, in_minutes: bool) -> String {
    if !in_minutes {
        return countdown(remaining);
    }
    let total = remaining.ceil().max(0.0) as i64;
    if total > 60 {
        let mins = (total + 59) / 60;
        match lang {
            Lang::Ko => format!("{mins}분"),
            Lang::En => format!("{mins}m"),
        }
    } else {
        match lang {
            Lang::Ko => format!("{total}초"),
            Lang::En => format!("{total}s"),
        }
    }
}

/// "오후 3:05" 또는 "3:05 PM" 같은 시각 표시 (이 컴퓨터의 시간대)
pub fn clock(ts: Timestamp) -> String {
    clock_in(i18n::current(), ts)
}

pub fn clock_in(lang: Lang, ts: Timestamp) -> String {
    match Local.timestamp_opt(ts.floor() as i64, 0).single() {
        Some(t) => ampm(lang, t.hour(), t.minute()),
        None => String::new(),
    }
}

/// 지정한 시간대(UTC 기준 초)로 시각 표시
pub fn clock_with_offset(ts: Timestamp, offset_seconds: i32) -> String {
    clock_with_offset_in(i18n::current(), ts, offset_seconds)
}

pub fn clock_with_offset_in(lang: Lang, ts: Timestamp, offset_seconds: i32) -> String {
    let tz = FixedOffset::east_opt(offset_seconds).unwrap_or(FixedOffset::east_opt(0).unwrap());
    match tz.timestamp_opt(ts.floor() as i64, 0).single() {
        Some(t) => ampm(lang, t.hour(), t.minute()),
        None => String::new(),
    }
}

fn ampm(lang: Lang, hour: u32, minute: u32) -> String {
    let (is_pm, h12) = match hour {
        0 => (false, 12),
        1..=11 => (false, hour),
        12 => (true, 12),
        _ => (true, hour - 12),
    };
    match lang {
        Lang::Ko => {
            let label = if is_pm { "오후" } else { "오전" };
            format!("{label} {h12}:{minute:02}")
        }
        Lang::En => {
            let label = if is_pm { "PM" } else { "AM" };
            format!("{h12}:{minute:02} {label}")
        }
    }
}

/// 메뉴·설정 창에 보여 줄 현재 상태 문구
pub fn state_text(state: &EngineState) -> String {
    state_text_in(i18n::current(), state)
}

pub fn state_text_in(lang: Lang, state: &EngineState) -> String {
    match lang {
        Lang::Ko => match state {
            EngineState::Disabled => "자동 잠금이 꺼져 있음".into(),
            EngineState::Paused { until: Some(t) } => format!("일시 중지됨 ({}까지)", clock_in(lang, *t)),
            EngineState::Paused { until: None } => "일시 중지됨 (다시 시작할 때까지)".into(),
            EngineState::NeverLock => "이 네트워크에서는 자동으로 잠그지 않음".into(),
            EngineState::HeldByAssertion { holders } => {
                let names: Vec<&str> = holders.iter().take(2).map(String::as_str).collect();
                format!("잠금 보류: {} 이(가) 화면 유지 중", names.join(", "))
            }
            EngineState::ScreenLocked => "화면 잠김".into(),
            EngineState::Counting { remaining, .. } => format!("잠금까지 {} 남음", countdown(*remaining)),
            EngineState::Warning { remaining } => format!("곧 잠김: {}초", remaining.ceil().max(0.0) as i64),
            EngineState::LockTriggered => "화면을 잠갔습니다".into(),
            EngineState::WaitingForActivity => "잠금 완료 · 다시 사용하면 시간을 새로 셉니다".into(),
            EngineState::IdleUnavailable => "이 데스크톱에서는 자리비움 시간을 읽을 수 없음".into(),
        },
        Lang::En => match state {
            EngineState::Disabled => "Auto-lock is off".into(),
            EngineState::Paused { until: Some(t) } => format!("Paused (until {})", clock_in(lang, *t)),
            EngineState::Paused { until: None } => "Paused (until resumed)".into(),
            EngineState::NeverLock => "Never auto-lock on this network".into(),
            EngineState::HeldByAssertion { holders } => {
                let names: Vec<&str> = holders.iter().take(2).map(String::as_str).collect();
                format!("Lock on hold: {} keeping screen awake", names.join(", "))
            }
            EngineState::ScreenLocked => "Screen locked".into(),
            EngineState::Counting { remaining, .. } => format!("Locks in {}", countdown(*remaining)),
            EngineState::Warning { remaining } => format!("Locking soon: {}s", remaining.ceil().max(0.0) as i64),
            EngineState::LockTriggered => "Screen locked".into(),
            EngineState::WaitingForActivity => "Locked · Timer resets on next activity".into(),
            EngineState::IdleUnavailable => "Cannot read idle time on this desktop".into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_formatting() {
        assert_eq!(timeout_in(Lang::Ko, 0), "잠그지 않음");
        assert_eq!(timeout_in(Lang::Ko, 30), "30초");
        assert_eq!(timeout_in(Lang::Ko, 90), "1분 30초");
        assert_eq!(timeout_in(Lang::Ko, 300), "5분");
        assert_eq!(timeout_in(Lang::Ko, 3600), "1시간");
        assert_eq!(timeout_in(Lang::Ko, 5400), "1시간 30분");
        assert_eq!(duration_in(Lang::Ko, 0), "0초");
        assert_eq!(countdown(59.2), "1:00");
        assert_eq!(countdown(0.0), "0:00");
        assert_eq!(countdown(-3.0), "0:00");
        assert_eq!(countdown(3725.0), "1:02:05");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 299.2, false), "5:00");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 299.2, true), "5분");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 240.5, true), "5분");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 240.0, true), "4분");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 61.0, true), "2분");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 60.0, true), "60초");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 9.3, true), "10초");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, -1.0, true), "0초");
        assert_eq!(menu_bar_countdown_in(Lang::Ko, 3725.0, true), "63분");
        // 1970-01-01 00:00 UTC = 서울 오전 9:00
        assert_eq!(clock_with_offset_in(Lang::Ko, 0.0, 9 * 3600), "오전 9:00");
        assert_eq!(clock_with_offset_in(Lang::Ko, 3.0 * 3600.0 + 5.0 * 60.0, 9 * 3600), "오후 12:05");
        assert_eq!(clock_with_offset_in(Lang::Ko, 15.0 * 3600.0, 9 * 3600), "오전 12:00");
        assert_eq!(clock_with_offset_in(Lang::Ko, 6.0 * 3600.0 + 7.0 * 60.0, 9 * 3600), "오후 3:07");
    }

    #[test]
    fn english_formatting() {
        assert_eq!(timeout_in(Lang::En, 0), "Never");
        assert_eq!(timeout_in(Lang::En, 30), "30 sec");
        assert_eq!(timeout_in(Lang::En, 90), "1 min 30 sec");
        assert_eq!(timeout_in(Lang::En, 300), "5 min");
        assert_eq!(timeout_in(Lang::En, 3600), "1 hr");
        assert_eq!(timeout_in(Lang::En, 5400), "1 hr 30 min");
        assert_eq!(duration_in(Lang::En, 0), "0 sec");
        assert_eq!(menu_bar_countdown_in(Lang::En, 299.2, false), "5:00");
        assert_eq!(menu_bar_countdown_in(Lang::En, 299.2, true), "5m");
        assert_eq!(menu_bar_countdown_in(Lang::En, 60.0, true), "60s");
        assert_eq!(menu_bar_countdown_in(Lang::En, 9.3, true), "10s");
        assert_eq!(clock_with_offset_in(Lang::En, 0.0, 9 * 3600), "9:00 AM");
        assert_eq!(clock_with_offset_in(Lang::En, 3.0 * 3600.0 + 5.0 * 60.0, 9 * 3600), "12:05 PM");
        assert_eq!(clock_with_offset_in(Lang::En, 15.0 * 3600.0, 9 * 3600), "12:00 AM");
        assert_eq!(clock_with_offset_in(Lang::En, 6.0 * 3600.0 + 7.0 * 60.0, 9 * 3600), "3:07 PM");
    }

    #[test]
    fn state_texts() {
        assert_eq!(state_text_in(Lang::Ko, &EngineState::Counting { remaining: 299.2, timeout: 300.0 }), "잠금까지 5:00 남음");
        assert_eq!(state_text_in(Lang::Ko, &EngineState::Warning { remaining: 4.1 }), "곧 잠김: 5초");
        assert_eq!(
            state_text_in(Lang::Ko, &EngineState::HeldByAssertion { holders: vec!["가".into(), "나".into(), "다".into()] }),
            "잠금 보류: 가, 나 이(가) 화면 유지 중"
        );
        assert_eq!(state_text_in(Lang::Ko, &EngineState::Disabled), "자동 잠금이 꺼져 있음");

        assert_eq!(state_text_in(Lang::En, &EngineState::Counting { remaining: 299.2, timeout: 300.0 }), "Locks in 5:00");
        assert_eq!(state_text_in(Lang::En, &EngineState::Warning { remaining: 4.1 }), "Locking soon: 5s");
        assert_eq!(
            state_text_in(Lang::En, &EngineState::HeldByAssertion { holders: vec!["VLC".into(), "Zoom".into(), "Obs".into()] }),
            "Lock on hold: VLC, Zoom keeping screen awake"
        );
        assert_eq!(state_text_in(Lang::En, &EngineState::Disabled), "Auto-lock is off");
        assert_eq!(state_text_in(Lang::En, &EngineState::Paused { until: None }), "Paused (until resumed)");
        assert_eq!(state_text_in(Lang::En, &EngineState::NeverLock), "Never auto-lock on this network");
        assert_eq!(state_text_in(Lang::En, &EngineState::ScreenLocked), "Screen locked");
        assert_eq!(state_text_in(Lang::En, &EngineState::WaitingForActivity), "Locked · Timer resets on next activity");
        assert_eq!(state_text_in(Lang::En, &EngineState::IdleUnavailable), "Cannot read idle time on this desktop");
    }
}
