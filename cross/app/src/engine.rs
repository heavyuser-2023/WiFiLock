//! 1초마다 시스템 상태를 읽어 잠금 여부를 판단하고 실행한다.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use wifilock_core::{EngineState, SystemInputs, TickResult};

use crate::logging;
use crate::state::{lock, state, AppState};
use crate::ui;

/// 판단 반복을 별도 스레드에서 시작한다.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("wifilock-engine".into())
        .spawn(move || {
            let mut n: u64 = 0;
            let mut last_kind = String::new();
            loop {
                let started = Instant::now();
                let st = state(&app);
                if !st.manual.load(Ordering::SeqCst) {
                    if n.is_multiple_of(2) {
                        refresh_wifi(&st);
                    }
                    if n.is_multiple_of(5) {
                        refresh_location(&st);
                    }
                    let r = tick(&app);
                    let kind = state_kind(&r.output.state);
                    if kind != last_kind {
                        logging::info(format!("상태: {}", wifilock_core::format::state_text(&r.output.state)));
                        last_kind = kind;
                    }
                }
                n = n.wrapping_add(1);
                let spent = started.elapsed();
                std::thread::sleep(Duration::from_millis(1000).saturating_sub(spent).max(Duration::from_millis(50)));
            }
        })
        .expect("판단 스레드 시작");
}

fn state_kind(s: &EngineState) -> String {
    let full = format!("{s:?}");
    full.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

/// 와이파이 상태를 다시 읽는다.
pub fn refresh_wifi(st: &AppState) {
    let snap = st.platform.wifi_snapshot();
    let mut w = lock(&st.wifi);
    if *w != snap {
        logging::info(format!(
            "와이파이: {} / 공유기 {}",
            snap.korean_description(),
            snap.router_mac.as_deref().unwrap_or("-")
        ));
        *w = snap;
    }
}

pub fn refresh_location(st: &AppState) {
    let s = st.platform.location_status();
    *lock(&st.location) = s;
}

/// 지금 시스템 값(또는 자가 진단의 가짜 값)
pub fn gather(st: &AppState) -> SystemInputs {
    if let Some(f) = lock(&st.fake).clone() {
        return SystemInputs {
            now: f.now,
            idle_seconds: Some(f.idle),
            screen_locked: f.locked,
            assertion_holders: f.holders,
            wifi: f.wifi,
        };
    }
    let respect = lock(&st.store).settings().respect_display_sleep_assertions;
    SystemInputs {
        now: wifilock_core::now(),
        idle_seconds: st.platform.idle_seconds(),
        screen_locked: st.platform.screen_locked(),
        assertion_holders: if respect { st.platform.display_sleep_preventers() } else { Vec::new() },
        wifi: lock(&st.wifi).clone(),
    }
}

/// 한 번 판단하고 화면을 갱신한다.
pub fn tick(app: &AppHandle) -> TickResult {
    let st = state(app);
    let inputs = gather(&st);
    tick_with(app, inputs)
}

pub fn tick_with(app: &AppHandle, inputs: SystemInputs) -> TickResult {
    let st = state(app);
    let now = inputs.now;
    let settings = lock(&st.store).settings().clone();
    let result = lock(&st.controller).tick(&settings, inputs);
    if let Some(reason) = &result.lock_reason {
        do_lock(app, reason, now);
    }
    ui::schedule_refresh(app);
    result
}

/// 화면을 잠근다 (시험 모드면 기록만).
pub fn do_lock(app: &AppHandle, reason: &str, now: f64) {
    let st = state(app);
    lock(&st.controller).did_lock(now);
    if st.opts.dry_run {
        let n = st.lock_count.fetch_add(1, Ordering::SeqCst) + 1;
        logging::info(format!("[시험 모드] 화면 잠금 요청 #{n}: {reason}"));
        return;
    }
    logging::info(format!("화면 잠금 실행: {reason}"));
    let platform = st.platform.clone();
    let run = move || match platform.lock_screen() {
        Ok(m) => logging::info(format!("잠금 방법: {m}")),
        Err(e) => logging::error(e),
    };
    // 맥의 잠금 함수는 주 스레드에서 부른다.
    #[cfg(target_os = "macos")]
    {
        if app.run_on_main_thread(run).is_err() {
            logging::error("주 스레드에서 잠금을 실행하지 못함");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        run();
    }
}

/// 사용자가 메뉴에서 바로 잠글 때
pub fn lock_now(app: &AppHandle) {
    let now = {
        let st = state(app);
        let f = lock(&st.fake).as_ref().map(|f| f.now);
        f.unwrap_or_else(wifilock_core::now)
    };
    do_lock(app, "사용자가 '지금 잠그기' 선택", now);
}
