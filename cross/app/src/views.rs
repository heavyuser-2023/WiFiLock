//! 설정 창과 잠금 직전 경고 창

use tauri::{AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::logging;
use crate::state::{lock, state};

pub const SETTINGS: &str = "settings";
pub const HUD: &str = "hud";
const HUD_W: f64 = 380.0;
const HUD_H: f64 = 84.0;

/// 설정 창을 연다 (이미 열려 있으면 앞으로 가져온다).
pub fn open_settings(app: &AppHandle) {
    activate_app(app);
    if let Some(w) = app.get_webview_window(SETTINGS) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings.html".into()))
        .title("와이파이 잠금 설정")
        .inner_size(640.0, 820.0)
        .min_inner_size(520.0, 480.0)
        .resizable(true)
        .center()
        .focused(true)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
            logging::info("설정 창 열기");
        }
        Err(e) => logging::error(format!("설정 창을 열지 못함: {e}")),
    }
}

/// 메뉴 막대 앱은 창을 열 때 앱을 앞으로 가져와야 창이 다른 앱 뒤에 숨지 않는다 (맥).
fn activate_app(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(|| unsafe {
            use objc2::msg_send;
            use objc2::runtime::{AnyClass, AnyObject};
            if let Some(cls) = AnyClass::get(c"NSApplication") {
                let ns_app: *mut AnyObject = msg_send![cls, sharedApplication];
                if !ns_app.is_null() {
                    let _: () = msg_send![ns_app, activateIgnoringOtherApps: true];
                }
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// 위치 권한을 요청한다 (맥은 주 스레드에서).
pub fn request_location(app: &AppHandle) {
    let st = state(app);
    let platform = st.platform.clone();
    activate_app(app);
    let _ = app.run_on_main_thread(move || {
        wifilock_platform::prepare_on_main_thread();
        platform.request_location_permission();
    });
}

/// 경고 창을 미리 만들어 둔다 (보이지 않게). 시작할 때 주 스레드에서 부른다.
pub fn create_hud(app: &AppHandle) {
    let built = WebviewWindowBuilder::new(app, HUD, WebviewUrl::App("hud.html".into()))
        .title("와이파이 잠금 경고")
        .inner_size(HUD_W, HUD_H)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        .visible_on_all_workspaces(true)
        .build();
    match built {
        Ok(w) => {
            // 리눅스의 창 라이브러리(tao)는 아직 그려지지 않은 창에 이 설정을 걸면 멈춘다 (tao 0.37).
            // 경고 창은 포커스를 받지 않으므로 리눅스에서는 생략한다.
            #[cfg(not(target_os = "linux"))]
            let _ = w.set_ignore_cursor_events(true);
            #[cfg(target_os = "macos")]
            configure_hud_macos(&w);
        }
        Err(e) => logging::error(format!("경고 창을 만들지 못함: {e}")),
    }
}

/// 맥: 전체 화면 앱 위·모든 데스크톱에 보이고, 마우스를 통과시키는 화면 보호기 수준의 창으로 만든다.
#[cfg(target_os = "macos")]
fn configure_hud_macos(w: &tauri::WebviewWindow) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    if let Ok(ptr) = w.ns_window() {
        let ns = ptr as *mut AnyObject;
        if ns.is_null() {
            return;
        }
        // canJoinAllSpaces(1) | stationary(16) | ignoresCycle(64) | fullScreenAuxiliary(256)
        let behavior: usize = 1 | 16 | 64 | 256;
        unsafe {
            let _: () = msg_send![ns, setLevel: 1000isize];
            let _: () = msg_send![ns, setCollectionBehavior: behavior];
            let _: () = msg_send![ns, setIgnoresMouseEvents: true];
            let _: () = msg_send![ns, setHidesOnDeactivate: false];
        }
    }
}

fn position_hud(app: &AppHandle, w: &tauri::WebviewWindow) {
    let monitor = app.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        let x = pos.x as f64 / scale + (size.width as f64 / scale - HUD_W) / 2.0;
        let y = pos.y as f64 / scale + 48.0;
        let _ = w.set_position(LogicalPosition::new(x, y));
    }
}

pub fn show_hud(app: &AppHandle, remaining: i64) {
    let st = state(app);
    let was = lock(&st.hud_remaining).replace(remaining);
    let Some(w) = app.get_webview_window(HUD) else { return };
    if was.is_none() {
        position_hud(app, &w);
        let _ = w.show();
        logging::info("잠금 직전 경고 표시");
    }
    if was != Some(remaining) {
        let _ = app.emit_to(HUD, "hud", remaining);
    }
}

pub fn hide_hud(app: &AppHandle) {
    let st = state(app);
    if lock(&st.hud_remaining).take().is_some() {
        if let Some(w) = app.get_webview_window(HUD) {
            let _ = w.hide();
        }
    }
}

pub fn hud_visible(app: &AppHandle) -> bool {
    app.get_webview_window(HUD).and_then(|w| w.is_visible().ok()).unwrap_or(false)
}
