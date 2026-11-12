//! 설정 창·경고 창에서 부르는 명령

use tauri::AppHandle;
use uuid::Uuid;
use wifilock_core::AppSettings;

use crate::engine;
use crate::logging;
use crate::state::{lock, state};
use crate::ui::{self, UiState};
use crate::views;

/// 설정을 바꾼 뒤 바로 판단·반영한다.
fn after_change(app: &AppHandle) -> UiState {
    let st = state(app);
    if !st.manual.load(std::sync::atomic::Ordering::SeqCst) {
        let inputs = engine::gather(&st);
        engine::tick_with(app, inputs);
    } else {
        ui::schedule_refresh(app);
    }
    ui::ui_state(app)
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> UiState {
    let st = state(&app);
    engine::refresh_wifi(&st);
    engine::refresh_location(&st);
    after_change(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: serde_json::Value) -> Result<UiState, String> {
    let parsed: AppSettings = serde_json::from_value(settings).map_err(|e| format!("설정 형식 오류: {e}"))?;
    let st = state(&app);
    if lock(&st.store).replace(parsed) {
        logging::info("설정 저장");
    }
    Ok(after_change(&app))
}

#[tauri::command]
pub fn add_current_network(app: AppHandle) -> UiState {
    let st = state(&app);
    let wifi = lock(&st.controller).status().snapshot.clone();
    if let Some(r) = lock(&st.store).add_rule_for(&wifi, None) {
        logging::info(format!("네트워크 등록: {}", r.name));
    }
    after_change(&app)
}

#[tauri::command]
pub fn add_rule_by_name(app: AppHandle, ssid: String, timeout: i64) -> Result<UiState, String> {
    let st = state(&app);
    let added = lock(&st.store).add_rule_ssid(&ssid, timeout);
    match added {
        Some(r) => {
            logging::info(format!("이름으로 네트워크 등록: {}", r.name));
            Ok(after_change(&app))
        }
        None if ssid.trim().is_empty() => Err("와이파이 이름을 입력해 주세요.".into()),
        None => Err("이미 같은 이름의 네트워크가 등록되어 있습니다.".into()),
    }
}

#[tauri::command]
pub fn remove_rule(app: AppHandle, id: String) -> UiState {
    if let Ok(id) = Uuid::parse_str(&id) {
        lock(&state(&app).store).remove_rule(id);
    }
    after_change(&app)
}

#[tauri::command]
pub fn move_rule(app: AppHandle, id: String, up: bool) -> UiState {
    if let Ok(id) = Uuid::parse_str(&id) {
        lock(&state(&app).store).move_rule(id, up);
    }
    after_change(&app)
}

#[tauri::command]
pub fn request_location(app: AppHandle) {
    views::request_location(&app);
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> UiState {
    let st = state(&app);
    let result = crate::set_autostart(&app, enabled);
    *lock(&st.autostart_error) = result.err();
    ui::ui_state(&app)
}

#[tauri::command]
pub fn hud_state(app: AppHandle) -> Option<i64> {
    *lock(&state(&app).hud_remaining)
}

/// 설정 창이 그려진 뒤 화면 정보를 보낸다 (자가 진단에서 확인).
#[tauri::command]
pub fn page_report(app: AppHandle, report: serde_json::Value) {
    *lock(&state(&app).page_report) = Some(report);
}
