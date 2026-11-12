//! 상태를 화면(메뉴 아이콘·메뉴·경고 창·설정 창)에 반영한다.
//!
//! 화면 반영은 모두 주 스레드에서 한다. 판단 스레드는 반영할 내용만 만들어 주 스레드로 보낸다.
//! (공유 상태의 잠금을 쥔 채 화면 함수를 부르면 주 스레드와 서로 기다리다 멈출 수 있어서다)

use serde::Serialize;
use tauri::image::Image;
use tauri::{AppHandle, Emitter, Manager};
use wifilock_core::format;
use wifilock_core::{AppSettings, EngineState, LockStatus, PolicyResolver, PolicySource, TimeoutPresets, WiFiSnapshot};
use wifilock_platform::LocationStatus;

use crate::menu;
use crate::state::{lock, state, AppState};
use crate::views;

pub const TRAY_ID: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconKind {
    Active,
    Inactive,
    Warning,
}

/// 메뉴 구조를 바꿔야 하는 값들 (바뀌면 메뉴를 새로 만든다)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuStructure {
    pub enabled: bool,
    pub paused: bool,
    pub show_permission: bool,
    pub add_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiSnapshot {
    pub icon: IconKind,
    pub tooltip: String,
    pub title: Option<String>,
    pub network: String,
    pub policy: String,
    pub state: String,
    pub structure: MenuStructure,
    /// Some 이면 경고 창에 보여 줄 남은 초
    pub hud: Option<i64>,
}

pub fn network_text(snap: &WiFiSnapshot) -> String {
    let mut s = format!("와이파이: {}", snap.korean_description());
    if snap.is_connected() && snap.ssid.is_none() {
        if let Some(mac) = &snap.router_mac {
            s.push_str(&format!(" · 공유기 {mac}"));
        }
    }
    s
}

pub fn policy_text(st: &LockStatus) -> String {
    let source = match &st.policy.source {
        PolicySource::Rule { rule } => format!("등록된 네트워크 '{}'", rule.name),
        PolicySource::UnknownWiFi => "등록되지 않은 네트워크".into(),
        PolicySource::NoWiFi => "와이파이 연결 없음".into(),
    };
    let pending = if st.policy_change_pending { " · 연결 변화 확인 중" } else { "" };
    format!("잠금 시간: {} ({source}){pending}", format::timeout(st.policy.timeout_seconds))
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

    let icon = match status.state {
        EngineState::Warning { .. } => IconKind::Warning,
        ref s if is_active(s) => IconKind::Active,
        _ => IconKind::Inactive,
    };
    let state_text = format::state_text(&status.state);
    let title = if settings.show_countdown_in_menu_bar && cfg!(any(target_os = "macos", target_os = "linux")) {
        match status.state {
            EngineState::Counting { remaining, .. } | EngineState::Warning { remaining } => Some(format::countdown(remaining)),
            _ => None,
        }
    } else {
        None
    };
    let add_label = if can_add_current(snap, &settings) {
        let name = snap
            .ssid
            .clone()
            .or_else(|| snap.router_mac.as_ref().map(|m| format!("공유기 {m}")))
            .unwrap_or_default();
        Some(format!("현재 네트워크({name})를 등록"))
    } else {
        None
    };
    let hud = match status.state {
        EngineState::Warning { remaining } if settings.warn_before_lock => Some(remaining.ceil().max(1.0) as i64),
        _ => None,
    };
    UiSnapshot {
        icon,
        tooltip: format!("와이파이 잠금 — {state_text}"),
        title,
        network: network_text(snap),
        policy: policy_text(&status),
        state: state_text,
        structure: MenuStructure {
            enabled: settings.enabled,
            paused: status.paused,
            show_permission: snap.is_connected() && !location.has_permission(),
            add_label,
        },
        hud,
    }
}

/// 판단 스레드 등에서 부른다: 반영할 내용을 만들어 주 스레드로 넘긴다.
pub fn schedule_refresh(app: &AppHandle) {
    let snap = snapshot(&state(app));
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || apply(&handle, snap));
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
        _ => include_bytes!("../icons/tray/mac-active.png"),
    };
    #[cfg(not(target_os = "macos"))]
    let bytes: &'static [u8] = match kind {
        IconKind::Active => include_bytes!("../icons/tray/color-active.png"),
        IconKind::Inactive => include_bytes!("../icons/tray/color-inactive.png"),
        IconKind::Warning => include_bytes!("../icons/tray/color-warning.png"),
    };
    Image::from_bytes(bytes).expect("아이콘 그림")
}

/// 주 스레드에서만 부른다.
fn apply(app: &AppHandle, snap: UiSnapshot) {
    let st = state(app);
    let prev = lock(&st.ui).clone();
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };

    if prev.as_ref().map(|p| p.icon) != Some(snap.icon) {
        let _ = tray.set_icon(Some(tray_image(snap.icon)));
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
    }

    match snap.hud {
        Some(r) => views::show_hud(app, r),
        None => views::hide_hud(app),
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
pub struct UiState {
    pub os: &'static str,
    pub version: String,
    pub dry_run: bool,
    pub settings: AppSettings,
    pub wifi: WiFiSnapshot,
    pub wifi_text: String,
    pub router_text: Option<String>,
    pub policy_text: String,
    pub state_text: String,
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
}

fn location_info(os: &str, status: LocationStatus, snap: &WiFiSnapshot) -> LocationInfo {
    let needs = snap.is_connected() && !status.has_permission();
    let (title, explanation, button) = match os {
        "macos" => {
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
        "windows" => (
            "위치 권한이 꺼져 있어 와이파이 이름을 읽을 수 없습니다".to_string(),
            "윈도우 11 24H2 부터는 위치 권한을 허용한 앱만 와이파이 이름을 읽을 수 있습니다. 설정 > 개인 정보 및 보안 > 위치에서 '위치 서비스'와 '데스크톱 앱이 위치에 액세스하도록 허용'을 켜 주세요. 그 전에는 공유기 주소로 네트워크를 구분합니다.".to_string(),
            "위치 설정 열기".to_string(),
        ),
        _ => (String::new(), String::new(), String::new()),
    };
    LocationInfo { status, needs_permission: needs, title, explanation, button }
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let st = state(app);
    let status = lock(&st.controller).status().clone();
    let settings = lock(&st.store).settings().clone();
    let location = *lock(&st.location);
    let wifi = status.snapshot.clone();
    let os = st.platform.os();

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
    let router_text = if wifi.is_connected() { wifi.router_mac.as_ref().map(|m| format!("공유기 주소 {m}")) } else { None };
    let holders_note = match os {
        "windows" => "윈도우에서는 어떤 앱이 화면 유지를 요청했는지 이름까지는 알 수 없습니다.",
        "linux" => "그놈·KDE 등 데스크톱이 알려 주는 화면 유지 요청을 따릅니다.",
        _ => "",
    }
    .to_string();
    let autostart = crate::autostart_enabled(app);
    let autostart_error = lock(&st.autostart_error).clone();

    UiState {
        os,
        version: app.package_info().version.to_string(),
        dry_run: st.opts.dry_run,
        can_add_current: can_add_current(&wifi, &settings),
        current_rule_id,
        location: location_info(os, location, &wifi),
        wifi_text: wifi.korean_description(),
        router_text,
        policy_text: format!(
            "지금 적용: {} · {}",
            format::timeout(status.policy.timeout_seconds),
            status.policy.source.korean_description()
        ),
        state_text: format::state_text(&status.state),
        wifi,
        settings,
        autostart,
        autostart_error,
        timeout_presets: TimeoutPresets::ALL.to_vec(),
        timeout_labels,
        warning_options,
        countdown_supported: cfg!(any(target_os = "macos", target_os = "linux")),
        holders_note,
    }
}
