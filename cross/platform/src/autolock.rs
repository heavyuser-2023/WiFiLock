//! 운영체제가 스스로 화면을 잠그는 설정 (화면 보호기, 디스플레이 끄기, 절전, 관리 정책).
//!
//! 이 앱과 운영체제는 따로 자리비움 시간을 재고 먼저 오는 쪽이 잠근다. 운영체제 쪽이 더 짧으면
//! 이 앱에서 길게 정한 시간이 효과가 없으므로 설정 창에서 알린다.
//! 여기에는 운영체제와 무관한 계산과 문구만 두고(모든 운영체제에서 시험), 값 읽기는 운영체제별 파일에서 한다.

use serde::Serialize;
use wifilock_core::{format, AppSettings};

/// 운영체제가 스스로 잠그는 조건 하나 (지금 전원 기준)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAutoLock {
    /// 입력이 없으면 이 시간(초) 뒤 운영체제가 스스로 잠근다
    pub after_seconds: i64,
    /// 왜 잠기는지 (예: "입력이 없으면 5분 뒤 화면 보호기가 시작되고 바로 암호를 요구합니다")
    pub cause: String,
    /// 바꾸는 곳 (예: "시스템 설정 > 잠금 화면의 '비활성 상태인 경우 화면 보호기 시작'").
    /// None 이면 관리자 정책이라 사용자가 바꿀 수 없다.
    pub setting: Option<String>,
    /// 그 설정 화면을 열 수 있으면 단추 글자
    pub button: Option<String>,
    /// 설정 화면 열기 대상 (운영체제별 구현이 해석한다)
    #[serde(skip)]
    pub target: Option<SettingsTarget>,
}

/// 열 수 있는 설정 화면
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTarget {
    MacLockScreen,
    WindowsScreenSaver,
    WindowsPower,
    KdeScreenLocker,
}

/// 잠금 시간이 짧은 것부터 늘어놓는다 (0 이하 = 잠그지 않음은 뺀다).
pub fn sorted(mut list: Vec<SystemAutoLock>) -> Vec<SystemAutoLock> {
    list.retain(|x| x.after_seconds > 0);
    list.sort_by_key(|x| x.after_seconds);
    list
}

/// "바로" 또는 "1분 뒤"
fn delay_text(delay: i64) -> String {
    if delay <= 0 {
        "바로".into()
    } else {
        format!("{} 뒤", format::duration(delay))
    }
}

// ---------- 설정 창 안내 ----------

/// 운영체제가 이 앱보다 먼저 잠그는 경우 설정 창에 보일 안내
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemLockNote {
    pub title: String,
    pub explanation: String,
    pub button: Option<String>,
}

/// 마지막 한글 글자에 받침이 있는지 (따옴표·괄호 등은 건너뛴다)
fn has_batchim(s: &str) -> bool {
    s.chars()
        .rev()
        .find(|c| ('가'..='힣').contains(c))
        .map(|c| !(c as u32 - '가' as u32).is_multiple_of(28))
        .unwrap_or(true)
}

/// 운영체제 잠금(`locks`, 짧은 것부터)이 이 앱에서 정한 잠금 시간보다 짧으면 안내를 만든다. 해당 없으면 None.
/// `os_name`: "맥", "윈도우", "데스크톱"
pub fn note(os_name: &str, locks: &[SystemAutoLock], settings: &AppSettings) -> Option<SystemLockNote> {
    if !settings.enabled {
        return None;
    }
    let first = locks.first()?;
    let affected = settings.timeouts_longer_than(first.after_seconds);
    if affected.is_empty() {
        return None;
    }
    let limit = format::duration(first.after_seconds);
    let list = affected.iter().map(|(n, t)| format!("{n} {}", format::timeout(*t))).collect::<Vec<_>>().join(", ");
    // 고쳐야 할 설정: 이 앱에서 정한 가장 긴 시간보다 먼저 잠그는 것 모두
    let never = affected.iter().any(|(_, t)| *t <= 0);
    let longest = affected.iter().map(|(_, t)| *t).max().unwrap_or(0);
    let blocking: Vec<&SystemAutoLock> = locks.iter().filter(|l| never || l.after_seconds < longest).collect();
    let mut names: Vec<&str> = blocking.iter().filter_map(|l| l.setting.as_deref()).collect();
    names.dedup();
    let mut fix = String::new();
    if !names.is_empty() {
        let joined = names.join(", ");
        let need = if never { "'안 함'으로".to_string() } else { format!("{} 이상으로", format::duration(longest)) };
        let object = if has_batchim(&joined) { "을" } else { "를" };
        fix.push_str(&format!(" 이 앱에서 정한 시간대로 잠그려면 {joined}{object} {need} 바꿔 주세요."));
    }
    if blocking.iter().any(|l| l.setting.is_none()) {
        fix.push_str(" 관리자가 정한 정책은 여기서 바꿀 수 없습니다.");
    }
    let (if_p, subject) = if has_batchim(&limit) { ("이면", "이") } else { ("면", "가") };
    Some(SystemLockNote {
        title: format!("{os_name} 설정 때문에 {limit}{if_p} 잠깁니다"),
        explanation: format!(
            "{}. 그래서 이 앱에서 더 길게 정한 잠금 시간도 실제로는 {limit}{subject} 됩니다: {list}.{fix}",
            first.cause
        ),
        button: blocking.iter().find_map(|l| l.button.clone()),
    })
}

// ---------- 맥 ----------

/// 맥 전원
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacPower {
    /// 전원 어댑터 (배터리가 있는 맥)
    Adapter,
    /// 전원 어댑터만 쓰는 맥 (데스크톱)
    AdapterOnly,
    Battery,
    Ups,
}

/// 맥이 스스로 잠그는 조건들.
/// - `screensaver_idle`: 화면 보호기 시작까지 초 (0·None = 시작 안 함)
/// - `display_sleep_minutes`: 지금 전원에서 디스플레이 끄기까지 분 (0·None = 끄지 않음)
/// - `password_delay`: 화면 보호기·디스플레이 꺼짐 뒤 암호 요구까지 초. None 이면 암호를 요구하지 않음
pub fn mac(
    screensaver_idle: Option<i64>,
    display_sleep_minutes: Option<i64>,
    power: MacPower,
    password_delay: Option<i64>,
) -> Vec<SystemAutoLock> {
    let Some(delay) = password_delay else { return Vec::new() };
    let mut out = Vec::new();
    let button = Some("잠금 화면 설정 열기".to_string());
    if let Some(s) = screensaver_idle.filter(|s| *s > 0) {
        out.push(SystemAutoLock {
            after_seconds: s + delay.max(0),
            cause: format!(
                "입력이 없으면 {} 뒤 화면 보호기가 시작되고 {} 암호를 요구합니다",
                format::duration(s),
                delay_text(delay)
            ),
            setting: Some("시스템 설정 > 잠금 화면의 '비활성 상태인 경우 화면 보호기 시작'".into()),
            button: button.clone(),
            target: Some(SettingsTarget::MacLockScreen),
        });
    }
    if let Some(m) = display_sleep_minutes.filter(|m| *m > 0) {
        let (row, now) = match power {
            MacPower::Battery => ("배터리 사용 시 비활성 상태인 경우 디스플레이 끄기", " (배터리 사용 중)"),
            MacPower::Adapter => ("전원 어댑터 사용 시 비활성 상태인 경우 디스플레이 끄기", " (전원 어댑터 사용 중)"),
            MacPower::AdapterOnly => ("비활성 상태인 경우 디스플레이 끄기", ""),
            MacPower::Ups => ("UPS 사용 시 비활성 상태인 경우 디스플레이 끄기", " (UPS 사용 중)"),
        };
        out.push(SystemAutoLock {
            after_seconds: m * 60 + delay.max(0),
            cause: format!(
                "입력이 없으면 {} 뒤 디스플레이가 꺼지고 {} 암호를 요구합니다{now}",
                format::duration(m * 60),
                delay_text(delay)
            ),
            setting: Some(format!("시스템 설정 > 잠금 화면의 '{row}'")),
            button,
            target: Some(SettingsTarget::MacLockScreen),
        });
    }
    sorted(out)
}

// ---------- 윈도우 ----------

/// 윈도우에서 읽은 값 (초 단위, 0 = 안 함)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsInputs {
    pub on_battery: bool,
    /// 화면 보호기가 정해져 있고 켜져 있으면 시작까지 초와 "다시 시작할 때 로그온 화면 표시" 여부
    pub screensaver: Option<(i64, bool)>,
    /// 관리 정책 "컴퓨터 비활성 한도" (초)
    pub inactivity_limit: Option<i64>,
    /// 지금 전원에서 화면 끄기까지 초
    pub display_off: Option<i64>,
    /// 지금 전원에서 절전까지 초
    pub sleep_after: Option<i64>,
    /// 지금 전원에서 대기·절전 뒤 다시 켤 때 로그인을 요구하는지와, 요구하기까지 더 기다리는 초.
    /// None 이면 로그인을 요구하지 않음
    pub wake_lock_delay: Option<i64>,
    /// 모던 스탠바이(화면이 꺼지면 대기 상태가 되는 컴퓨터)
    pub modern_standby: bool,
}

pub fn windows(i: &WindowsInputs) -> Vec<SystemAutoLock> {
    let mut out = Vec::new();
    let power_now = if i.on_battery { " (배터리 사용 중)" } else { " (전원 연결 중)" };
    let power_setting = "설정 > 시스템 > 전원 및 배터리의 '화면, 절전 및 최대 절전 시간 제한'";
    if let Some((t, true)) = i.screensaver.filter(|(t, _)| *t > 0) {
        out.push(SystemAutoLock {
            after_seconds: t,
            cause: format!("입력이 없으면 {} 뒤 화면 보호기가 시작되고, 다시 쓰려면 로그인해야 합니다", format::duration(t)),
            setting: Some("설정 > 개인 설정 > 잠금 화면 > 화면 보호기의 '대기' 시간".into()),
            button: Some("화면 보호기 설정 열기".into()),
            target: Some(SettingsTarget::WindowsScreenSaver),
        });
    }
    if let Some(t) = i.inactivity_limit.filter(|t| *t > 0) {
        out.push(SystemAutoLock {
            after_seconds: t,
            cause: format!("회사 보안 정책(컴퓨터 비활성 한도)에 따라 {} 동안 입력이 없으면 잠깁니다", format::duration(t)),
            setting: None,
            button: None,
            target: None,
        });
    }
    if let Some(d) = i.wake_lock_delay {
        let d = d.max(0);
        let signin = if d == 0 { "다시 켤 때 로그인해야 합니다".to_string() } else { format!("{} 뒤부터는 다시 켤 때 로그인해야 합니다", format::duration(d)) };
        if let Some(t) = i.display_off.filter(|t| *t > 0 && i.modern_standby) {
            out.push(SystemAutoLock {
                after_seconds: t + d,
                cause: format!("입력이 없으면 {} 뒤 화면이 꺼지며 대기 상태가 되고, {signin}{power_now}", format::duration(t)),
                setting: Some(format!("{power_setting} 중 화면 끄기")),
                button: Some("전원 설정 열기".into()),
                target: Some(SettingsTarget::WindowsPower),
            });
        }
        if let Some(t) = i.sleep_after.filter(|t| *t > 0) {
            out.push(SystemAutoLock {
                after_seconds: t + d,
                cause: format!("입력이 없으면 {} 뒤 절전 모드로 들어가고, {signin}{power_now}", format::duration(t)),
                setting: Some(format!("{power_setting} 중 절전")),
                button: Some("전원 설정 열기".into()),
                target: Some(SettingsTarget::WindowsPower),
            });
        }
    }
    sorted(out)
}

// ---------- 리눅스 ----------

/// 그놈: idle-delay(빈 화면까지 초), lock-enabled, lock-delay(빈 화면 뒤 잠금까지 초)
pub fn gnome(idle_delay: i64, lock_enabled: bool, lock_delay: i64) -> Vec<SystemAutoLock> {
    if !lock_enabled || idle_delay <= 0 {
        return Vec::new();
    }
    vec![SystemAutoLock {
        after_seconds: idle_delay + lock_delay.max(0),
        cause: format!(
            "입력이 없으면 {} 뒤 화면이 꺼지고 {} 잠깁니다",
            format::duration(idle_delay),
            delay_text(lock_delay)
        ),
        setting: Some("설정 > 개인 정보 및 보안 > 화면 잠금의 '빈 화면 지연'(예전 그놈은 설정 > 전원의 '빈 화면')".into()),
        button: None,
        target: None,
    }]
}

/// KDE: 화면 자동 잠금 사용, 잠금까지 분, 잠긴 뒤 암호 없이 풀 수 있는 유예 초
pub fn kde(autolock: bool, timeout_minutes: i64, grace_seconds: i64) -> Vec<SystemAutoLock> {
    if !autolock || timeout_minutes <= 0 {
        return Vec::new();
    }
    vec![SystemAutoLock {
        after_seconds: timeout_minutes * 60 + grace_seconds.max(0),
        cause: format!("입력이 없으면 {} 뒤 화면을 잠급니다", format::duration(timeout_minutes * 60)),
        setting: Some("시스템 설정 > 화면 잠금의 '화면 자동 잠금'".into()),
        button: Some("화면 잠금 설정 열기".into()),
        target: Some(SettingsTarget::KdeScreenLocker),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_screensaver_and_display() {
        let v = mac(Some(300), Some(60), MacPower::Adapter, Some(0));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].after_seconds, 300);
        assert_eq!(v[0].cause, "입력이 없으면 5분 뒤 화면 보호기가 시작되고 바로 암호를 요구합니다");
        assert_eq!(v[1].after_seconds, 3600);
        assert!(v[1].setting.as_deref().unwrap().contains("전원 어댑터 사용 시"));
        assert!(v[1].cause.ends_with("(전원 어댑터 사용 중)"));

        let v = mac(Some(1200), Some(5), MacPower::Battery, Some(60));
        assert_eq!(v[0].after_seconds, 360, "디스플레이 5분 + 암호 요구 1분");
        assert_eq!(v[0].cause, "입력이 없으면 5분 뒤 디스플레이가 꺼지고 1분 뒤 암호를 요구합니다 (배터리 사용 중)");
        assert!(v[0].setting.as_deref().unwrap().contains("'배터리 사용 시 비활성 상태인 경우 디스플레이 끄기'"));
    }

    #[test]
    fn mac_without_password_or_timers() {
        assert!(mac(Some(300), Some(5), MacPower::Battery, None).is_empty(), "암호를 요구하지 않으면 잠기지 않음");
        assert!(mac(Some(0), Some(0), MacPower::Adapter, Some(0)).is_empty(), "안 함");
        assert!(mac(None, None, MacPower::AdapterOnly, Some(0)).is_empty());
        let v = mac(None, Some(10), MacPower::AdapterOnly, Some(0));
        assert_eq!(v[0].setting.as_deref(), Some("시스템 설정 > 잠금 화면의 '비활성 상태인 경우 디스플레이 끄기'"));
    }

    #[test]
    fn windows_sources() {
        let i = WindowsInputs {
            on_battery: false,
            screensaver: Some((600, true)),
            inactivity_limit: Some(900),
            display_off: Some(300),
            sleep_after: Some(1800),
            wake_lock_delay: Some(0),
            modern_standby: false,
        };
        let v = windows(&i);
        assert_eq!(v.iter().map(|x| x.after_seconds).collect::<Vec<_>>(), vec![600, 900, 1800], "모던 스탠바이가 아니면 화면 끄기로는 잠기지 않음");
        assert!(v[1].setting.is_none(), "관리 정책은 바꿀 수 없음");

        let v = windows(&WindowsInputs { modern_standby: true, ..i.clone() });
        assert_eq!(v[0].after_seconds, 300);
        assert!(v[0].cause.ends_with("(전원 연결 중)"));

        let v = windows(&WindowsInputs { modern_standby: true, wake_lock_delay: Some(60), ..i.clone() });
        assert_eq!(v[0].after_seconds, 360, "화면 끄기 5분 + 로그인 요구까지 1분");
        assert!(v[0].cause.contains("1분 뒤부터는 다시 켤 때 로그인해야 합니다"));

        let v = windows(&WindowsInputs { wake_lock_delay: None, screensaver: Some((600, false)), inactivity_limit: None, ..i });
        assert!(v.is_empty(), "로그인 요구가 없으면 잠기지 않음");
    }

    fn settings_with(rules: &[(&str, i64)], unknown: i64, nowifi: i64) -> AppSettings {
        let mut s = AppSettings { unknown_wifi_timeout_seconds: unknown, no_wifi_timeout_seconds: nowifi, ..AppSettings::default() };
        for (name, t) in rules {
            s.rules.push(wifilock_core::NetworkRule {
                id: Default::default(),
                name: name.to_string(),
                ssid: Some(name.to_string()),
                router_mac: None,
                timeout_seconds: *t,
            });
        }
        s
    }

    #[test]
    fn note_for_shorter_system_lock() {
        let s = settings_with(&[("우리집", 3600), ("카페", 300)], 300, 180);
        let locks = mac(Some(300), Some(60), MacPower::Adapter, Some(0));
        let n = note("맥", &locks, &s).expect("우리집 1시간이 5분이 됨");
        assert_eq!(n.title, "맥 설정 때문에 5분이면 잠깁니다");
        assert_eq!(
            n.explanation,
            "입력이 없으면 5분 뒤 화면 보호기가 시작되고 바로 암호를 요구합니다. 그래서 이 앱에서 더 길게 정한 잠금 시간도 실제로는 5분이 됩니다: 우리집 1시간. \
             이 앱에서 정한 시간대로 잠그려면 시스템 설정 > 잠금 화면의 '비활성 상태인 경우 화면 보호기 시작'을 1시간 이상으로 바꿔 주세요."
        );
        assert_eq!(n.button.as_deref(), Some("잠금 화면 설정 열기"));

        // 배터리: 화면 보호기와 디스플레이 끄기 둘 다 5분이면 둘 다 고치라고 알린다
        let locks = mac(Some(300), Some(5), MacPower::Battery, Some(0));
        let n = note("맥", &locks, &s).unwrap();
        assert!(n.explanation.contains("'비활성 상태인 경우 화면 보호기 시작', 시스템 설정 > 잠금 화면의 '배터리 사용 시 비활성 상태인 경우 디스플레이 끄기'를 1시간 이상으로"));

        // 잠그지 않음 규칙은 '안 함'으로
        let s2 = settings_with(&[("서재", 0)], 300, 180);
        assert!(note("맥", &locks, &s2).unwrap().explanation.contains("'안 함'으로 바꿔 주세요"));
    }

    #[test]
    fn no_note_when_system_lock_is_longer_or_app_off() {
        let s = settings_with(&[("우리집", 1800)], 300, 180);
        assert!(note("맥", &mac(Some(3600), Some(60), MacPower::Adapter, Some(0)), &s).is_none());
        assert!(note("맥", &[], &s).is_none());
        let off = AppSettings { enabled: false, ..s.clone() };
        assert!(note("맥", &mac(Some(300), None, MacPower::Adapter, Some(0)), &off).is_none());
    }

    #[test]
    fn note_particles_and_policy() {
        let s = settings_with(&[("회사", 3600)], 300, 180);
        let n = note("데스크톱", &gnome(30, true, 0), &s).unwrap();
        assert_eq!(n.title, "데스크톱 설정 때문에 30초면 잠깁니다");
        assert!(n.explanation.contains("실제로는 30초가 됩니다"));
        let policy = windows(&WindowsInputs { inactivity_limit: Some(900), ..Default::default() });
        let n = note("윈도우", &policy, &s).unwrap();
        assert!(n.explanation.ends_with("관리자가 정한 정책은 여기서 바꿀 수 없습니다."));
        assert_eq!(n.button, None);
    }

    #[test]
    fn linux_desktops() {
        let v = gnome(300, true, 0);
        assert_eq!(v[0].after_seconds, 300);
        assert_eq!(v[0].cause, "입력이 없으면 5분 뒤 화면이 꺼지고 바로 잠깁니다");
        assert_eq!(gnome(300, true, 30)[0].after_seconds, 330);
        assert!(gnome(300, false, 0).is_empty());
        assert!(gnome(0, true, 0).is_empty());
        assert_eq!(kde(true, 5, 5)[0].after_seconds, 305);
        assert!(kde(false, 5, 5).is_empty());
    }
}
