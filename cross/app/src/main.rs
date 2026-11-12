// 윈도우에서 릴리스 빌드는 콘솔 창 없이 실행한다.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod engine;
mod logging;
mod menu;
mod options;
mod selftest;
mod state;
mod ui;
mod views;

use std::sync::Arc;

use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use wifilock_core::SettingsStore;

use crate::options::Options;
use crate::state::AppState;

pub fn autostart_enabled(app: &AppHandle) -> bool {
    app.try_state::<tauri_plugin_autostart::AutoLaunchManager>()
        .and_then(|m| m.is_enabled().ok())
        .unwrap_or(false)
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let m = app.autolaunch();
    let r = if enabled { m.enable() } else { m.disable() };
    match r {
        Ok(()) => {
            logging::info(format!("로그인할 때 자동 실행 {}", if enabled { "켬" } else { "끔" }));
            Ok(())
        }
        Err(e) => {
            let msg = format!("자동 실행 설정을 바꾸지 못했습니다: {e}");
            logging::error(&msg);
            Err(msg)
        }
    }
}

fn main() {
    let opts = Options::parse(std::env::args());
    let is_self_test = opts.is_self_test();

    let mut builder = tauri::Builder::default();
    if !is_self_test {
        // 이미 실행 중인데 다시 열면 (파인더·시작 메뉴에서 다시 실행 등) 설정 창을 띄운다.
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            logging::info("이미 실행 중 → 설정 창 열기");
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || views::open_settings(&handle));
        }));
    }

    let app = builder
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_settings,
            commands::add_current_network,
            commands::add_rule_by_name,
            commands::remove_rule,
            commands::move_rule,
            commands::request_location,
            commands::set_autostart,
            commands::hud_state,
            commands::page_report,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // 기록 파일
            let log_path = opts.log_file.clone().unwrap_or_else(|| {
                handle.path().app_log_dir().unwrap_or_else(|_| std::env::temp_dir()).join("wifilock.log")
            });
            logging::init(&log_path);
            logging::info(format!(
                "시작: 버전 {}, {} , 시험 모드 {}, 자가 진단 {}",
                handle.package_info().version,
                std::env::consts::OS,
                opts.dry_run,
                is_self_test
            ));

            // 설정 파일
            let settings_path = match (&opts.self_test_dir, &opts.settings_file) {
                (Some(dir), _) => {
                    let p = dir.join("settings.json");
                    let _ = std::fs::create_dir_all(dir);
                    let _ = std::fs::remove_file(&p);
                    p
                }
                (None, Some(p)) => p.clone(),
                (None, None) => handle
                    .path()
                    .app_config_dir()
                    .unwrap_or_else(|_| std::env::temp_dir().join("wifilock"))
                    .join("settings.json"),
            };
            let first_launch = !settings_path.exists() && !is_self_test;
            let store = SettingsStore::open(&settings_path);
            if first_launch {
                store.save();
            }

            let platform: Arc<dyn wifilock_platform::Platform> = Arc::from(wifilock_platform::create());
            let st = Arc::new(AppState::new(opts.clone(), platform, store));
            app.manage(st.clone());

            // 메뉴 막대 전용 앱: 독에 나타나지 않게 한다.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // 위치 관리자는 주 스레드에서 만들어 둔다 (맥).
            wifilock_platform::prepare_on_main_thread();

            // 처음 상태를 읽어 아이콘·메뉴를 만든다.
            engine::refresh_wifi(&st);
            engine::refresh_location(&st);
            let first = ui::snapshot(&st);
            let (menu, handles) = menu::build(&handle, &first)?;
            *state::lock(&st.menu) = Some(handles);
            let tray = TrayIconBuilder::with_id(ui::TRAY_ID)
                .icon(ui::tray_image(first.icon))
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip(&first.tooltip)
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| menu::handle(app, event.id().as_ref()))
                .build(app)?;
            drop(tray);
            *state::lock(&st.ui) = Some(first);

            views::create_hud(&handle);
            engine::start(handle.clone());

            if let Some(dir) = opts.self_test_dir.clone() {
                selftest::start(handle.clone(), dir);
                return Ok(());
            }

            if opts.preview_hud {
                // 판단 반복이 경고 창을 바로 닫지 않도록 멈춘다.
                st.manual.store(true, std::sync::atomic::Ordering::SeqCst);
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(800));
                    let h2 = h.clone();
                    let _ = h.run_on_main_thread(move || views::show_hud(&h2, 9));
                });
            }

            // 처음 실행이면 설정 창을 보여 주고 위치 권한을 요청한다.
            if first_launch || opts.open_settings {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(600));
                    let h2 = h.clone();
                    let _ = h.run_on_main_thread(move || views::open_settings(&h2));
                });
            }
            if !opts.no_location_prompt && st.platform.location_status() == wifilock_platform::LocationStatus::NotDetermined {
                let h = handle.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                    views::request_location(&h);
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("앱 시작 실패");

    app.run(|_app, event| match event {
        // 설정 창을 닫아도 앱은 계속 메뉴 막대에 남는다.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        RunEvent::Exit => {
            logging::info("종료");
        }
        _ => {}
    });
}
