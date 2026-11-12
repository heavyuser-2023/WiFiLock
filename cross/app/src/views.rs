//! 설정 창과 잠금 직전 경고 창

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

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
        // 마우스 통과 설정(set_ignore_cursor_events)은 쓰지 않는다:
        // 리눅스에서는 아직 그려지지 않은 창에 걸면 앱이 멈추고(tao 0.37),
        // 윈도우에서는 창이 계층 창이 되어 그려지지 않을 수 있다. 맥은 아래에서 직접 설정한다.
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
        Ok(w) => {
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

/// 주 모니터 위쪽 가운데.
/// - 윈도우: 물리 좌표 그대로 쓰므로 모니터마다 배율이 달라도 맞다.
/// - 맥·리눅스: 화면 전체 좌표가 논리(포인트) 단위라 논리 좌표로 넘긴다.
///   (맥의 창 라이브러리는 물리 좌표를 "창이 지금 있는 화면"의 배율로 나누므로, 주 화면을 바꾼 직후 어긋난다)
fn position_hud(app: &AppHandle, w: &tauri::WebviewWindow) {
    let monitor = app.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    if let Some(m) = monitor {
        let scale = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        #[cfg(windows)]
        {
            let x = pos.x as f64 + (size.width as f64 - HUD_W * scale) / 2.0;
            let y = pos.y as f64 + 48.0 * scale;
            let _ = w.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
        }
        #[cfg(not(windows))]
        {
            let x = pos.x as f64 / scale + (size.width as f64 / scale - HUD_W) / 2.0;
            let y = pos.y as f64 / scale + 48.0;
            let _ = w.set_position(tauri::LogicalPosition::new(x, y));
        }
    }
}

/// 윈도우: 키보드 초점을 빼앗지 않도록 활성화 없이 보이고, 맨 위에 둔다.
///
/// 주의: 경고 창의 보이기·숨기기를 여기서 직접 하므로 창 라이브러리(tao)는 이 창을 늘 "숨김"으로 알고 있다.
/// 그래서 경고 창에는 tao 의 창 상태 표시를 바꾸는 호출(set_always_on_top, set_focusable,
/// set_ignore_cursor_events, show, hide 등)을 하지 않는다. 하면 tao 가 창을 숨겨 버릴 수 있다.
/// 경고 창은 마우스를 통과시키지 않는다 (통과시키려면 계층 창이 되어 그려지지 않을 수 있어서다).
/// 경고 창 자리를 누르면 그 클릭은 아래 창으로 가지 않지만, 마우스를 움직인 순간 경고는 취소된다.
#[cfg(windows)]
fn win32_show(w: &tauri::WebviewWindow, show: bool) -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    };
    let Ok(h) = w.hwnd() else { return false };
    let hwnd = HWND(h.0);
    unsafe {
        if show {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
    true
}

fn set_hud_visible(w: &tauri::WebviewWindow, show: bool) {
    #[cfg(windows)]
    if win32_show(w, show) {
        return;
    }
    let _ = if show { w.show() } else { w.hide() };
}

pub fn show_hud(app: &AppHandle, remaining: i64) {
    let st = state(app);
    let was = lock(&st.hud_remaining).replace(remaining);
    let Some(w) = app.get_webview_window(HUD) else { return };
    if was.is_none() {
        position_hud(app, &w);
        set_hud_visible(&w, true);
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
            set_hud_visible(&w, false);
        }
    }
}

pub fn hud_visible(app: &AppHandle) -> bool {
    let Some(w) = app.get_webview_window(HUD) else { return false };
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;
        if let Ok(h) = w.hwnd() {
            return unsafe { IsWindowVisible(HWND(h.0)).as_bool() };
        }
    }
    w.is_visible().unwrap_or(false)
}
