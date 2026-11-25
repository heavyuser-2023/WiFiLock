//! 상태를 화면(메뉴 아이콘·메뉴·경고 창·설정 창)에 반영한다.
//!
//! 화면 반영은 모두 주 스레드에서 한다. 판단 스레드는 반영할 내용만 만들어 주 스레드로 보낸다.
//! (공유 상태의 잠금을 쥔 채 화면 함수를 부르면 주 스레드와 서로 기다리다 멈출 수 있어서다)

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::image::Image;
use tauri::{AppHandle, Emitter, Manager};
use wifilock_core::{format, i18n, Lang};
use wifilock_core::{
    AppSettings, BluetoothDeviceLive, EngineState, LockStatus, PolicyResolver, PolicySource, ProximityRange,
    TimeoutPresets, WiFiSnapshot,
};
use wifilock_platform::{BluetoothStatus, LocationStatus, SystemLockNote};

use crate::menu;
use crate::state::{lock, state, AppState};
pub use crate::tray_badge::{BadgeUnit, TrayBadge};
use crate::views;

pub const TRAY_ID: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Active,
    Inactive,
    Warning,
    /// 다른 앱이 화면을 켜 두어 잠금을 보류 중 (켜진 화면 + 열린 자물쇠)
    Held,
}

/// 메뉴 구조를 바꿔야 하는 값들 (바뀌면 메뉴를 새로 만든다)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuStructure {
    pub lang: Option<Lang>,
    pub enabled: bool,
    pub paused: bool,
    /// 잠금 보류 중이면 그 까닭과 안내 줄을 더한다
    pub held: bool,
    pub show_permission: bool,
    pub add_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSnapshot {
    pub icon: IconKind,
    pub icon_badge: TrayBadge,
    pub tooltip: String,
    pub title: Option<String>,
    pub network: String,
    pub policy: String,
    pub state: String,
    /// 잠금 보류 중일 때 화면 유지를 요청한 앱 또는 근처 블루투스 기기 줄 (아니면 빈 글자)
    pub holders: String,
    /// 잠금 보류 해제 안내 줄
    pub held_hint: String,
    pub structure: MenuStructure,
    /// Some 이면 경고 창에 보여 줄 남은 초
    pub hud: Option<i64>,
    /// 잠금이 가까워 경고 창을 미리 준비해 둘 때
    pub near_lock: bool,
}

pub fn network_text(snap: &WiFiSnapshot) -> String {
    let ko = i18n::is_ko();
    let mut s = if ko {
        format!("와이파이: {}", snap.korean_description())
    } else {
        format!("Wi-Fi: {}", snap.korean_description())
    };
    if snap.is_connected() && snap.ssid.is_none() {
        if let Some(mac) = &snap.router_mac {
            s.push_str(&if ko { format!(" · 공유기 {mac}") } else { format!(" · Router {mac}") });
        }
    }
    s
}

pub fn policy_text(st: &LockStatus) -> String {
    let ko = i18n::is_ko();
    let source = match (&st.policy.source, ko) {
        (PolicySource::Rule { rule }, true) => format!("등록된 네트워크 '{}'", rule.name),
        (PolicySource::UnknownWiFi, true) => "등록되지 않은 네트워크".into(),
        (PolicySource::NoWiFi, true) => "와이파이 연결 없음".into(),
        (PolicySource::Rule { rule }, false) => format!("Registered network '{}'", rule.name),
        (PolicySource::UnknownWiFi, false) => "Unregistered network".into(),
        (PolicySource::NoWiFi, false) => "No Wi-Fi connection".into(),
    };
    let pending = if st.policy_change_pending {
        i18n::pick(" · 연결 변화 확인 중", " · Checking connection change")
    } else {
        ""
    };
    let time = format::timeout(st.policy.timeout_seconds);
    if ko {
        format!("잠금 시간: {time} ({source}){pending}")
    } else {
        format!("Lock Time: {time} ({source}){pending}")
    }
}

/// 현재 네트워크를 등록할 수 있는지 (이미 등록됐으면 false, 이름을 몰랐던 규칙에 이름을 채울 수 있으면 true)
pub fn can_add_current(snap: &WiFiSnapshot, settings: &AppSettings) -> bool {
    if !snap.is_connected() || (snap.ssid.is_none() && snap.router_mac.is_none()) {
        return false;
    }
    match PolicyResolver::matching_rule(snap, &settings.rules) {
        None => true,
        Some(r) => snap.ssid.is_some() && r.ssid.is_none(),
    }
}

fn is_active(state: &EngineState) -> bool {
    !matches!(state, EngineState::Disabled | EngineState::Paused { .. } | EngineState::NeverLock | EngineState::IdleUnavailable)
}

pub fn snapshot(st: &AppState) -> UiSnapshot {
    let status = lock(&st.controller).status().clone();
    let settings = lock(&st.store).settings().clone();
    let location = *lock(&st.location);
    let snap = &status.snapshot;

    let held_lines = match &status.state {
        EngineState::HeldByAssertion { holders } => Some((format::held_holders(holders), format::held_hint())),
        EngineState::HeldByBluetooth { devices } => Some((format::held_bt_devices(devices), format::held_bt_hint())),
        _ => None,
    };
    let icon = match status.state {
        EngineState::Warning { .. } => IconKind::Warning,
        EngineState::HeldByAssertion { .. } | EngineState::HeldByBluetooth { .. } => IconKind::Held,
        ref s if is_active(s) => IconKind::Active,
        _ => IconKind::Inactive,
    };
    let icon_badge = TrayBadge::compute(&status.state, &settings, i18n::current(), !cfg!(target_os = "macos"));
    // 보류 중이면 메뉴 첫 줄은 "잠기지 않음"만 쓰고, 사유는 다음 줄에 따로 보인다.
    let state_text = match &held_lines {
        Some(_) => format::held_headline(),
        None => format::state_text(&status.state),
    };
    let policy = policy_text(&status);
    let ko = i18n::is_ko();
    let app_name = i18n::pick("와이파이 잠금", "WiFiLock");
    // 마우스를 올렸을 때 보이는 도움말에는 매 초 바뀌는 남은 시간을 넣지 않는다.
    // 도움말을 바꿀 때마다 메뉴 막대가 아이콘 크기를 다시 계산하기 때문이다 (남은 시간은 메뉴에서 보인다).
    let tooltip = match status.state {
        EngineState::Counting { .. } => format!("{app_name} — {policy}"),
        EngineState::Warning { .. } => format!("{app_name} — {}", i18n::pick("곧 잠김", "Locking soon")),
        ref other => format!("{app_name} — {}", format::state_text(other)),
    };
    // 메뉴 막대 글자는 바뀔 때만 다시 그린다 (apply). 분 단위면 마지막 1분 전까지는 1분에 한 번만 바뀐다.
    // 잠금 보류는 남은 시간 표시를 꺼 두었어도 글자로 알린다 (아이콘만으로는 놓치기 쉬워서).
    // 윈도우·리눅스는 알림 영역 아이콘 자체에도 남은 시간/보류 배지를 함께 그린다.
    let title = if cfg!(any(target_os = "macos", target_os = "linux")) {
        match status.state {
            EngineState::HeldByAssertion { .. } | EngineState::HeldByBluetooth { .. } => Some(format::menu_bar_held()),
            EngineState::Counting { remaining, .. } | EngineState::Warning { remaining } if settings.show_countdown_in_menu_bar => {
                Some(format::menu_bar_countdown(remaining, settings.countdown_in_minutes))
            }
            _ => None,
        }
    } else {
        None
    };
    let add_label = if can_add_current(snap, &settings) {
        let name = snap
            .ssid
            .clone()
            .or_else(|| snap.router_mac.as_ref().map(|m| if ko { format!("공유기 {m}") } else { format!("Router {m}") }))
            .unwrap_or_default();
        Some(if ko {
            format!("현재 네트워크({name})를 등록")
        } else {
            format!("Add Current Network ({name})")
        })
    } else {
        None
    };
    let hud = match status.state {
        EngineState::Warning { remaining } if settings.warn_before_lock => Some(remaining.ceil().max(1.0) as i64),
        _ => None,
    };
    let near_lock = match status.state {
        EngineState::Counting { remaining, timeout } => {
            settings.warn_before_lock
                && remaining <= wifilock_core::LockScheduler::effective_warning(timeout, settings.warning_seconds as f64) + 10.0
        }
        _ => false,
    };
    let is_held = held_lines.is_some();
    let (holders, held_hint) = held_lines.unwrap_or_default();
    UiSnapshot {
        icon,
        icon_badge,
        tooltip,
        title,
        network: network_text(snap),
        policy,
        state: state_text,
        holders,
        held_hint,
        structure: MenuStructure {
            lang: Some(i18n::current()),
            enabled: settings.enabled,
            paused: status.paused,
            held: is_held,
            show_permission: snap.is_connected() && !location.has_permission(),
            add_label,
        },
        hud,
        near_lock,
    }
}

/// 판단 스레드 등에서 부른다: 주 스레드에서 그때의 최신 상태로 반영한다.
/// (여기서 미리 계산해 넘기면 그 사이 메뉴에서 바꾼 내용을 오래된 상태로 덮어쓸 수 있다)
pub fn schedule_refresh(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || refresh_now(&handle));
}

/// 주 스레드에서 지금 바로 반영한다.
pub fn refresh_now(app: &AppHandle) {
    let snap = snapshot(&state(app));
    apply(app, snap);
}

pub fn tray_image(kind: IconKind) -> Image<'static> {
    #[cfg(target_os = "macos")]
    let bytes: &'static [u8] = match kind {
        IconKind::Inactive => include_bytes!("../icons/tray/mac-inactive.png"),
        IconKind::Held => include_bytes!("../icons/tray/mac-held.png"),
        IconKind::Active | IconKind::Warning => include_bytes!("../icons/tray/mac-active.png"),
    };
    #[cfg(not(target_os = "macos"))]
    let bytes: &'static [u8] = match kind {
        IconKind::Active => include_bytes!("../icons/tray/color-active.png"),
        IconKind::Inactive => include_bytes!("../icons/tray/color-inactive.png"),
        IconKind::Warning => include_bytes!("../icons/tray/color-warning.png"),
        IconKind::Held => include_bytes!("../icons/tray/color-held.png"),
    };
    Image::from_bytes(bytes).expect("아이콘 그림")
}

/// 동적 배지(`Held` / `Countdown`)는 64×64 RGBA 배지를 생성하고, 정적 상태는 기본 아이콘을 반환한다.
pub fn tray_image_for_badge(badge: &TrayBadge) -> Image<'static> {
    crate::tray_badge::render_badge(badge).unwrap_or_else(|| tray_image(badge.fallback_icon()))
}

/// 주 스레드에서만 부른다.
fn apply(app: &AppHandle, snap: UiSnapshot) {
    let st = state(app);
    let prev = lock(&st.ui).clone();
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };

    if prev.as_ref().map(|p| &p.icon_badge) != Some(&snap.icon_badge) {
        let _ = tray.set_icon(Some(tray_image_for_badge(&snap.icon_badge)));
        #[cfg(target_os = "macos")]
        let _ = tray.set_icon_as_template(true);
    }
    if prev.as_ref().map(|p| &p.tooltip) != Some(&snap.tooltip) {
        let _ = tray.set_tooltip(Some(&snap.tooltip));
    }
    if prev.as_ref().map(|p| &p.title) != Some(&snap.title) {
        let _ = tray.set_title(snap.title.as_deref());
    }

    let rebuild = prev.as_ref().map(|p| &p.structure) != Some(&snap.structure) || lock(&st.menu).is_none();
    if rebuild {
        match menu::build(app, &snap) {
            Ok((m, handles)) => {
                let _ = tray.set_menu(Some(m));
                *lock(&st.menu) = Some(handles);
            }
            Err(e) => crate::logging::error(format!("메뉴 만들기 실패: {e}")),
        }
    } else if let Some(h) = lock(&st.menu).as_ref() {
        let p = prev.as_ref();
        if p.map(|p| &p.network) != Some(&snap.network) {
            let _ = h.network.set_text(&snap.network);
        }
        if p.map(|p| &p.policy) != Some(&snap.policy) {
            let _ = h.policy.set_text(&snap.policy);
        }
        if p.map(|p| &p.state) != Some(&snap.state) {
            let _ = h.state.set_text(&snap.state);
        }
        if p.map(|p| &p.holders) != Some(&snap.holders) {
            let _ = h.holders.set_text(&snap.holders);
        }
        if p.map(|p| &p.held_hint) != Some(&snap.held_hint) {
            let _ = h.held_hint.set_text(&snap.held_hint);
        }
    }

    match snap.hud {
        Some(r) => views::show_hud(app, r),
        None => {
            views::hide_hud(app);
            views::manage_idle_hud(app, snap.near_lock);
        }
    }

    if app.get_webview_window(views::SETTINGS).is_some() {
        let _ = app.emit_to(views::SETTINGS, "state", ui_state(app));
    }
    *lock(&st.ui) = Some(snap);
}

// ---------- 설정 창에 보내는 전체 상태 ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub status: LocationStatus,
    pub needs_permission: bool,
    pub title: String,
    pub explanation: String,
    pub button: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub value: i64,
    pub label: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothNotice {
    pub title: String,
    pub explanation: String,
    pub button: String,
    pub action: &'static str,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RangeChoice {
    pub value: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub enter_rssi: i16,
    pub exit_rssi: i16,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothUiInfo {
    pub status: BluetoothStatus,
    pub scanning: bool,
    pub notice: Option<BluetoothNotice>,
    pub live: Vec<BluetoothDeviceLive>,
    pub range_options: Vec<RangeChoice>,
    pub enter_rssi: i16,
    pub exit_rssi: i16,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub lang: &'static str,
    pub os: &'static str,
    pub version: String,
    pub dry_run: bool,
    pub settings: AppSettings,
    pub wifi: WiFiSnapshot,
    pub wifi_text: String,
    pub router_text: Option<String>,
    pub policy_text: String,
    pub state_text: String,
    pub is_held: bool,
    pub can_add_current: bool,
    pub current_rule_id: Option<String>,
    pub location: LocationInfo,
    pub autostart: bool,
    pub autostart_error: Option<String>,
    /// 잠금 시간 선택지 (값 → 글자). 규칙마다 현재 값이 목록에 없으면 끼워 넣어 쓴다.
    pub timeout_presets: Vec<i64>,
    pub timeout_labels: Vec<Choice>,
    pub warning_options: Vec<Choice>,
    pub countdown_supported: bool,
    pub holders_note: String,
    /// 운영체제가 이 앱보다 먼저 잠그면 그 안내
    pub system_lock: Option<SystemLockNote>,
    /// 블루투스 근접 보류 UI 상태
    pub bluetooth: BluetoothUiInfo,
}

fn location_info(os: &str, status: LocationStatus, snap: &WiFiSnapshot) -> LocationInfo {
    let needs = snap.is_connected() && !status.has_permission();
    let ko = i18n::is_ko();
    let (title, explanation, button) = match (os, ko) {
        ("macos", true) => {
            let head = "macOS 는 위치 권한을 받은 앱에만 와이파이 이름을 알려 줍니다. ";
            let tail = " 위치 정보는 와이파이 이름 확인에만 쓰며 저장하거나 밖으로 보내지 않습니다.";
            let mid = if snap.router_mac.is_some() {
                "지금은 공유기 주소로 네트워크를 구분하고 있지만, 와이파이 이름으로 구분하려면 권한을 허용해 주세요."
            } else {
                "권한이 없으면 '와이파이 연결됨'과 '연결 안 됨'만 구분해 기본 잠금 시간을 적용합니다. 집·사무실처럼 네트워크마다 시간을 다르게 하려면 권한을 허용해 주세요."
            };
            (
                "위치 권한이 없어 와이파이 이름을 읽을 수 없습니다".to_string(),
                format!("{head}{mid}{tail}"),
                if status == LocationStatus::NotDetermined { "위치 권한 요청" } else { "시스템 설정에서 허용하기" }.to_string(),
            )
        }
        ("macos", false) => {
            let head = "macOS only shares Wi-Fi network names with apps that have Location permission. ";
            let tail = " Location data is used solely to check the Wi-Fi name and is never stored or sent anywhere.";
            let mid = if snap.router_mac.is_some() {
                "Currently distinguishing networks by router MAC address; grant permission to identify networks by Wi-Fi name."
            } else {
                "Without permission, only 'Wi-Fi connected' and 'Not connected' are distinguished using default timeouts. Grant permission to set per-network timeouts."
            };
            (
                "Location permission required to read Wi-Fi name".to_string(),
                format!("{head}{mid}{tail}"),
                if status == LocationStatus::NotDetermined { "Request Location Permission" } else { "Allow in System Settings" }.to_string(),
            )
        }
        ("windows", true) => (
            "위치 권한이 꺼져 있어 와이파이 이름을 읽을 수 없습니다".to_string(),
            if wifilock_platform::is_packaged() {
                "윈도우 11 24H2 부터는 위치 권한을 허용한 앱만 와이파이 이름을 읽을 수 있습니다. 설정 > 개인 정보 및 보안 > 위치에서 '위치 서비스'와 '앱이 위치에 액세스하도록 허용'에서 WiFiLock을 켜 주세요. 그 전에는 공유기 주소로 네트워크를 구분합니다."
            } else {
                "윈도우 11 24H2 부터는 위치 권한을 허용한 앱만 와이파이 이름을 읽을 수 있습니다. 설정 > 개인 정보 및 보안 > 위치에서 '위치 서비스'와 '데스크톱 앱이 위치에 액세스하도록 허용'을 켜 주세요. 그 전에는 공유기 주소로 네트워크를 구분합니다."
            }
            .to_string(),
            "위치 설정 열기".to_string(),
        ),
        ("windows", false) => (
            "Location permission is off; cannot read Wi-Fi name".to_string(),
            if wifilock_platform::is_packaged() {
                "Starting with Windows 11 24H2, apps need Location permission to read Wi-Fi names. Turn on 'Location services' and allow WiFiLock under 'Let apps access your location' in Settings > Privacy & security > Location. Until then, networks are identified by router MAC address."
            } else {
                "Starting with Windows 11 24H2, apps need Location permission to read Wi-Fi names. Turn on 'Location services' and 'Let desktop apps access your location' under Settings > Privacy & security > Location. Until then, networks are identified by router MAC address."
            }
            .to_string(),
            "Open Location Settings".to_string(),
        ),
        _ => (String::new(), String::new(), String::new()),
    };
    LocationInfo { status, needs_permission: needs, title, explanation, button }
}

fn bluetooth_notice(status: BluetoothStatus, relevant: bool, lang: Lang) -> Option<BluetoothNotice> {
    if !relevant {
        return None;
    }
    let ko = lang == Lang::Ko;
    match status {
        BluetoothStatus::Ready => None,
        BluetoothStatus::NotDetermined => Some(BluetoothNotice {
            title: if ko { "블루투스 권한이 필요합니다" } else { "Bluetooth permission required" }.into(),
            explanation: if ko {
                "지정한 기기가 근처에 있는지 확인하려면 블루투스 권한을 허용해 주세요. 주변 기기 정보는 저장하거나 밖으로 보내지 않습니다."
            } else {
                "Allow Bluetooth access to detect nearby trusted devices. Device data is never stored or sent anywhere."
            }
            .into(),
            button: if ko { "블루투스 권한 요청" } else { "Request Permission" }.into(),
            action: "permission",
        }),
        BluetoothStatus::Unauthorized => Some(BluetoothNotice {
            title: if ko { "블루투스 권한이 거절되어 있습니다" } else { "Bluetooth permission denied" }.into(),
            explanation: if ko {
                "시스템 설정의 개인정보 보호 및 보안 > 블루투스에서 WiFiLock을 허용해 주세요."
            } else {
                "Please allow WiFiLock under System Settings > Privacy & Security > Bluetooth."
            }
            .into(),
            button: if ko { "시스템 설정에서 허용하기" } else { "Allow in System Settings" }.into(),
            action: "settings",
        }),
        BluetoothStatus::PoweredOff => Some(BluetoothNotice {
            title: if ko { "블루투스가 꺼져 있습니다" } else { "Bluetooth is turned off" }.into(),
            explanation: if ko {
                "블루투스가 꺼져 있어 주변 기기를 감지할 수 없습니다."
            } else {
                "Bluetooth is turned off, so nearby devices cannot be detected."
            }
            .into(),
            button: if ko { "블루투스 설정 열기" } else { "Open Bluetooth Settings" }.into(),
            action: "settings",
        }),
        BluetoothStatus::Unavailable => Some(BluetoothNotice {
            title: if ko { "사용 가능한 블루투스 장치가 없습니다" } else { "No Bluetooth adapter available" }.into(),
            explanation: if ko {
                "이 컴퓨터에서 블루투스 어댑터를 찾지 못했습니다."
            } else {
                "Could not find a Bluetooth adapter on this computer."
            }
            .into(),
            button: String::new(),
            action: "",
        }),
    }
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let st = state(app);
    let status = lock(&st.controller).status().clone();
    let settings = lock(&st.store).settings().clone();
    let location = *lock(&st.location);
    let wifi = status.snapshot.clone();
    let os = st.platform.os();
    let ko = i18n::is_ko();

    let mut values: Vec<i64> = TimeoutPresets::ALL.to_vec();
    values.push(settings.unknown_wifi_timeout_seconds);
    values.push(settings.no_wifi_timeout_seconds);
    values.extend(settings.rules.iter().map(|r| r.timeout_seconds));
    values.sort_unstable();
    values.dedup();
    let timeout_labels = values.into_iter().map(|v| Choice { value: v, label: format::timeout(v) }).collect();
    let warning_options = TimeoutPresets::warning_including(settings.warning_seconds)
        .into_iter()
        .map(|v| Choice { value: v, label: format::duration(v) })
        .collect();

    let current_rule_id = PolicyResolver::matching_rule(&wifi, &settings.rules).map(|r| r.id.to_string());
    let router_text = if wifi.is_connected() {
        wifi.router_mac
            .as_ref()
            .map(|m| if ko { format!("공유기 주소 {m}") } else { format!("Router MAC {m}") })
    } else {
        None
    };
    let holders_note = match (os, ko) {
        ("windows", true) => "윈도우에서는 어떤 앱이 화면 유지를 요청했는지 이름까지는 알 수 없습니다.",
        ("windows", false) => "On Windows, the specific app keeping the screen awake cannot be identified.",
        ("linux", true) => "그놈·KDE 등 데스크톱이 알려 주는 화면 유지 요청을 따릅니다.",
        ("linux", false) => "Follows display sleep requests reported by GNOME, KDE, or systemd.",
        _ => "",
    }
    .to_string();
    let autostart = crate::autostart_enabled(app);
    let autostart_error = lock(&st.autostart_error).clone();
    let os_name = match (os, ko) {
        ("macos", true) => "맥",
        ("macos", false) => "macOS",
        ("windows", true) => "윈도우",
        ("windows", false) => "Windows",
        (_, true) => "데스크톱",
        (_, false) => "Desktop",
    };
    let system_lock = wifilock_platform::autolock::note(os_name, &lock(&st.system_locks).1, &settings);

    let lang = i18n::current();
    let bt_status = *lock(&st.bt_status);
    let bt_scanning = st.bt_radar_active.load(Ordering::Relaxed);
    let bt_relevant = settings.bluetooth_hold_enabled || !settings.bluetooth_devices.is_empty() || bt_scanning;
    let (enter_rssi, exit_rssi) = settings.effective_bluetooth_rssi();
    let range_options = ProximityRange::ALL
        .iter()
        .map(|r| RangeChoice {
            value: r.as_str(),
            label: r.label_in(lang),
            description: r.description_in(lang),
            enter_rssi: r.enter_rssi(),
            exit_rssi: r.exit_rssi(),
        })
        .collect();
    let bluetooth = BluetoothUiInfo {
        status: bt_status,
        scanning: bt_scanning,
        notice: bluetooth_notice(bt_status, bt_relevant, lang),
        live: status.bt_live.clone(),
        range_options,
        enter_rssi,
        exit_rssi,
    };

    let is_held = matches!(
        status.state,
        EngineState::HeldByAssertion { .. } | EngineState::HeldByBluetooth { .. }
    );

    UiState {
        lang: i18n::code(),
        os,
        version: app.package_info().version.to_string(),
        dry_run: st.opts.dry_run,
        can_add_current: can_add_current(&wifi, &settings),
        current_rule_id,
        location: location_info(os, location, &wifi),
        wifi_text: wifi.korean_description(),
        router_text,
        policy_text: if ko {
            format!("지금 적용: {} · {}", format::timeout(status.policy.timeout_seconds), status.policy.source.korean_description())
        } else {
            format!("Current policy: {} · {}", format::timeout(status.policy.timeout_seconds), status.policy.source.korean_description())
        },
        state_text: format::state_text(&status.state),
        is_held,
        wifi,
        settings,
        autostart,
        autostart_error,
        timeout_presets: TimeoutPresets::ALL.to_vec(),
        timeout_labels,
        warning_options,
        countdown_supported: true,
        holders_note,
        system_lock,
        bluetooth,
    }
}
