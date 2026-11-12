//! 앱 전체가 함께 쓰는 상태

use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex, MutexGuard};

use tauri::{AppHandle, Manager};
use wifilock_core::{Controller, SettingsStore, WiFiSnapshot};
use wifilock_platform::{LocationStatus, Platform};

use crate::menu::MenuHandles;
use crate::options::Options;
use crate::ui::UiSnapshot;

/// 자가 진단에서 넣는 가짜 시스템 값
#[derive(Debug, Clone, Default)]
pub struct FakeInputs {
    pub now: f64,
    pub idle: f64,
    pub locked: bool,
    pub holders: Vec<String>,
    pub wifi: WiFiSnapshot,
}

pub struct AppState {
    pub opts: Options,
    pub platform: Arc<dyn Platform>,
    pub store: Mutex<SettingsStore>,
    pub controller: Mutex<Controller>,
    pub wifi: Mutex<WiFiSnapshot>,
    pub location: Mutex<LocationStatus>,
    /// Some 이면 시스템 대신 이 값으로 판단한다 (자가 진단)
    pub fake: Mutex<Option<FakeInputs>>,
    /// true 면 1초 판단 반복을 멈춘다 (자가 진단이 직접 한 박자씩 진행)
    pub manual: AtomicBool,
    /// 시험 모드에서 잠금 요청 횟수
    pub lock_count: AtomicUsize,
    /// 화면에 마지막으로 반영한 상태 (주 스레드에서만 사용)
    pub ui: Mutex<Option<UiSnapshot>>,
    pub menu: Mutex<Option<MenuHandles>>,
    /// 경고 창에 보여 줄 남은 초
    pub hud_remaining: Mutex<Option<i64>>,
    /// 설정 창이 열리며 보낸 화면 정보 (자가 진단 확인용)
    pub page_report: Mutex<Option<serde_json::Value>>,
    pub autostart_error: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(opts: Options, platform: Arc<dyn Platform>, store: SettingsStore) -> Self {
        let override_timeout = opts.override_timeout;
        AppState {
            opts,
            platform,
            store: Mutex::new(store),
            controller: Mutex::new(Controller::new(override_timeout)),
            wifi: Mutex::new(WiFiSnapshot::unknown()),
            location: Mutex::new(LocationStatus::NotRequired),
            fake: Mutex::new(None),
            manual: AtomicBool::new(false),
            lock_count: AtomicUsize::new(0),
            ui: Mutex::new(None),
            menu: Mutex::new(None),
            hud_remaining: Mutex::new(None),
            page_report: Mutex::new(None),
            autostart_error: Mutex::new(None),
        }
    }
}

/// 잠금이 풀리지 않고 남아 있어도(다른 스레드가 멈춤) 앱 전체가 멈추지 않도록 독을 무시한다.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn state(app: &AppHandle) -> Arc<AppState> {
    app.state::<Arc<AppState>>().inner().clone()
}
