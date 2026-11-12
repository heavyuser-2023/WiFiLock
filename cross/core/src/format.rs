//! 한글 시간 표기와 상태 문구

use chrono::{FixedOffset, Local, TimeZone, Timelike};

use crate::scheduler::EngineState;
use crate::Timestamp;

/// 잠금 시간 표시: 0 → "잠그지 않음", 90 → "1분 30초", 3600 → "1시간"
pub fn timeout(seconds: i64) -> String {
    if seconds <= 0 {
        return "잠그지 않음".into();
    }
    duration(seconds)
}

/// 기간 표시: 45 → "45초", 120 → "2분", 5400 → "1시간 30분"
pub fn duration(seconds: i64) -> String {
    let s = seconds.max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h}시간"));
    }
    if m > 0 {
        parts.push(format!("{m}분"));
    }
    if sec > 0 || parts.is_empty() {
        parts.push(format!("{sec}초"));
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

/// "오후 3:05" 같은 시각 표시 (이 컴퓨터의 시간대)
pub fn clock(ts: Timestamp) -> String {
    match Local.timestamp_opt(ts.floor() as i64, 0).single() {
        Some(t) => ampm(t.hour(), t.minute()),
        None => String::new(),
    }
}

/// 지정한 시간대(UTC 기준 초)로 시각 표시
pub fn clock_with_offset(ts: Timestamp, offset_seconds: i32) -> String {
    let tz = FixedOffset::east_opt(offset_seconds).unwrap_or(FixedOffset::east_opt(0).unwrap());
    match tz.timestamp_opt(ts.floor() as i64, 0).single() {
        Some(t) => ampm(t.hour(), t.minute()),
        None => String::new(),
    }
}

fn ampm(hour: u32, minute: u32) -> String {
    let (label, h12) = match hour {
        0 => ("오전", 12),
        1..=11 => ("오전", hour),
        12 => ("오후", 12),
        _ => ("오후", hour - 12),
    };
    format!("{label} {h12}:{minute:02}")
}

/// 메뉴·설정 창에 보여 줄 현재 상태 문구
pub fn state_text(state: &EngineState) -> String {
    match state {
        EngineState::Disabled => "자동 잠금이 꺼져 있음".into(),
        EngineState::Paused { until: Some(t) } => format!("일시 중지됨 ({}까지)", clock(*t)),
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn korean_formatting() {
        assert_eq!(timeout(0), "잠그지 않음");
        assert_eq!(timeout(30), "30초");
        assert_eq!(timeout(90), "1분 30초");
        assert_eq!(timeout(300), "5분");
        assert_eq!(timeout(3600), "1시간");
        assert_eq!(timeout(5400), "1시간 30분");
        assert_eq!(duration(0), "0초");
        assert_eq!(countdown(59.2), "1:00");
        assert_eq!(countdown(0.0), "0:00");
        assert_eq!(countdown(-3.0), "0:00");
        assert_eq!(countdown(3725.0), "1:02:05");
        // 1970-01-01 00:00 UTC = 서울 오전 9:00
        assert_eq!(clock_with_offset(0.0, 9 * 3600), "오전 9:00");
        assert_eq!(clock_with_offset(3.0 * 3600.0 + 5.0 * 60.0, 9 * 3600), "오후 12:05");
        assert_eq!(clock_with_offset(15.0 * 3600.0, 9 * 3600), "오전 12:00");
        assert_eq!(clock_with_offset(6.0 * 3600.0 + 7.0 * 60.0, 9 * 3600), "오후 3:07");
    }

    #[test]
    fn state_texts() {
        assert_eq!(state_text(&EngineState::Counting { remaining: 299.2, timeout: 300.0 }), "잠금까지 5:00 남음");
        assert_eq!(state_text(&EngineState::Warning { remaining: 4.1 }), "곧 잠김: 5초");
        assert_eq!(
            state_text(&EngineState::HeldByAssertion { holders: vec!["가".into(), "나".into(), "다".into()] }),
            "잠금 보류: 가, 나 이(가) 화면 유지 중"
        );
        assert_eq!(state_text(&EngineState::Disabled), "자동 잠금이 꺼져 있음");
    }
}
