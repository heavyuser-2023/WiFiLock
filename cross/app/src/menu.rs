//! 메뉴 막대(알림 영역) 아이콘의 메뉴

use tauri::menu::{AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Wry};

use crate::engine;
use crate::logging;
use crate::state::{lock, state};
use crate::ui::{self, UiSnapshot};
use crate::views;

pub struct MenuHandles {
    pub network: MenuItem<Wry>,
    pub policy: MenuItem<Wry>,
    pub state: MenuItem<Wry>,
    pub enabled: CheckMenuItem<Wry>,
    /// 지금 메뉴에 보이는 최상위 항목 식별자들 (자가 진단 확인용)
    pub ids: Vec<String>,
}

pub const PAUSE_CHOICES: [(&str, u64); 4] = [("30분", 1800), ("1시간", 3600), ("2시간", 7200), ("4시간", 14400)];

/// 맥에서는 단축키 표시를 붙인다 (메뉴가 열려 있을 때만 동작).
fn accel(key: &str) -> Option<String> {
    if cfg!(target_os = "macos") {
        Some(format!("CmdOrCtrl+{key}"))
    } else {
        None
    }
}

pub fn build(app: &AppHandle, snap: &UiSnapshot) -> tauri::Result<(Menu<Wry>, MenuHandles)> {
    let s = &snap.structure;
    let network = MenuItem::with_id(app, "status-network", &snap.network, false, None::<&str>)?;
    let policy = MenuItem::with_id(app, "status-policy", &snap.policy, false, None::<&str>)?;
    let state_item = MenuItem::with_id(app, "status-state", &snap.state, false, None::<&str>)?;
    let permission = MenuItem::with_id(app, "permission", "와이파이 이름을 읽으려면 위치 권한이 필요합니다…", true, None::<&str>)?;
    let lock_now = MenuItem::with_id(app, "lock-now", "지금 잠그기", true, accel("L"))?;
    let enabled = CheckMenuItem::with_id(app, "enabled", "자동 잠금 사용", true, s.enabled, None::<&str>)?;

    let mut pause_items: Vec<MenuItem<Wry>> = Vec::new();
    for (label, secs) in PAUSE_CHOICES {
        pause_items.push(MenuItem::with_id(app, format!("pause-{secs}"), label, true, None::<&str>)?);
    }
    let forever = MenuItem::with_id(app, "pause-forever", "다시 시작할 때까지", true, None::<&str>)?;
    let sep_p = PredefinedMenuItem::separator(app)?;
    let mut pause_refs: Vec<&dyn IsMenuItem<Wry>> = pause_items.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    pause_refs.push(&sep_p);
    pause_refs.push(&forever);
    let pause = Submenu::with_id_and_items(app, "pause", "일시 중지", true, &pause_refs)?;
    let resume = MenuItem::with_id(app, "resume", "다시 시작", true, None::<&str>)?;
    let add = s
        .add_label
        .as_ref()
        .map(|label| MenuItem::with_id(app, "add-network", label, true, None::<&str>))
        .transpose()?;
    let settings = MenuItem::with_id(app, "settings", "설정…", true, accel(","))?;
    let about = PredefinedMenuItem::about(
        app,
        Some("와이파이 잠금 정보"),
        Some(AboutMetadata {
            name: Some("와이파이 잠금".into()),
            version: Some(app.package_info().version.to_string()),
            comments: Some(
                "와이파이 연결 상태에 따라 자동 잠금 시간을 바꿔 주는 앱입니다.\n등록한 네트워크에서는 길게, 낯선 곳에서는 짧게 잠급니다.".into(),
            ),
            license: Some("MIT".into()),
            website: Some("https://github.com/heavyuser-2023/WiFiLock".into()),
            ..Default::default()
        }),
    )?;
    let quit = MenuItem::with_id(app, "quit", "종료", true, accel("Q"))?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;

    let mut items: Vec<&dyn IsMenuItem<Wry>> = Vec::new();
    let mut ids: Vec<String> = Vec::new();
    macro_rules! add {
        ($item:expr, $id:expr) => {{
            items.push($item);
            ids.push($id.to_string());
        }};
    }
    add!(&network, "status-network");
    add!(&policy, "status-policy");
    add!(&state_item, "status-state");
    if s.show_permission {
        add!(&permission, "permission");
    }
    add!(&sep1, "---");
    add!(&lock_now, "lock-now");
    add!(&enabled, "enabled");
    if s.paused {
        add!(&resume, "resume");
    } else if s.enabled {
        add!(&pause, "pause");
    }
    add!(&sep2, "---");
    if let Some(a) = &add {
        add!(a, "add-network");
    }
    add!(&settings, "settings");
    add!(&sep3, "---");
    add!(&about, "about");
    add!(&quit, "quit");

    let menu = Menu::with_items(app, &items)?;
    Ok((menu, MenuHandles { network, policy, state: state_item, enabled, ids }))
}

/// 메뉴 항목을 골랐을 때 (주 스레드)
pub fn handle(app: &AppHandle, id: &str) {
    let st = state(app);
    let now = engine::logic_now(&st);
    match id {
        "lock-now" => engine::lock_now(app),
        "enabled" => {
            let on = {
                let mut s = lock(&st.store);
                s.update(|x| x.enabled = !x.enabled);
                s.settings().enabled
            };
            logging::info(format!("자동 잠금 {}", if on { "켬" } else { "끔" }));
        }
        "pause-forever" => {
            lock(&st.controller).pause_for(now, None);
            logging::info("일시 중지: 다시 시작할 때까지");
        }
        "resume" => {
            lock(&st.controller).resume();
            logging::info("다시 시작");
        }
        "add-network" => {
            let wifi = lock(&st.controller).status().snapshot.clone();
            if let Some(rule) = lock(&st.store).add_rule_for(&wifi, None) {
                logging::info(format!("네트워크 등록: {}", rule.name));
            }
            views::open_settings(app);
        }
        "permission" => views::request_location(app),
        "settings" => views::open_settings(app),
        "quit" => {
            logging::info("메뉴에서 종료");
            app.exit(0);
            return;
        }
        other => {
            if let Some(secs) = other.strip_prefix("pause-").and_then(|s| s.parse::<f64>().ok()) {
                lock(&st.controller).pause_for(now, Some(secs));
                logging::info(format!("일시 중지: {}초", secs as i64));
            } else {
                return;
            }
        }
    }
    // 바뀐 내용을 바로 반영하고, 판단은 따로 돌린다.
    ui::refresh_now(app);
    engine::tick_soon(app);
}
