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
            let mut forced = false;
            loop {
                let started = Instant::now();
                let st = state(&app);
                if !st.manual.load(Ordering::SeqCst) {
                    if forced || n.is_multiple_of(2) {
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
                forced = wait_next(&st, started + Duration::from_millis(1000));
            }
        })
        .expect("판단 스레드 시작");
}

/// 다음 판단 시각까지 기다린다. 그 전에 신호가 오면 바로 돌아간다 (돌아간 이유가 신호면 true).
fn wait_next(st: &AppState, deadline: Instant) -> bool {
    let (m, cv) = &st.wake;
    let mut woken = lock(m);
    loop {
        if *woken {
            *woken = false;
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        woken = cv.wait_timeout(woken, deadline - now).map(|(g, _)| g).unwrap_or_else(|e| e.into_inner().0);
    }
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

/// 판단에 쓰는 지금 시각 (잠자기 동안에도 흐르는 단조 시계, 자가 진단이면 가짜 시각)
pub fn logic_now(st: &AppState) -> f64 {
    if let Some(f) = lock(&st.fake).as_ref() {
        return f.now;
    }
    wifilock_platform::clocks().total
}

/// 지금 시스템 값(또는 자가 진단의 가짜 값)
pub fn gather(st: &AppState) -> SystemInputs {
    if let Some(f) = lock(&st.fake).clone() {
        return SystemInputs {
            now: f.now,
            awake: f.now,
            wall: f.now,
            idle_seconds: Some(f.idle),
            screen_locked: f.locked,
            assertion_holders: f.holders,
            wifi: f.wifi,
        };
    }
    let respect = lock(&st.store).settings().respect_display_sleep_assertions;
    let clocks = wifilock_platform::clocks();
    SystemInputs {
        now: clocks.total,
        awake: clocks.awake,
        wall: wifilock_core::now(),
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
    // 맥의 잠금 함수는 주 스레드에서 부른다 (바로 끝난다).
    #[cfg(target_os = "macos")]
    {
        if app.run_on_main_thread(run).is_err() {
            logging::error("주 스레드에서 잠금을 실행하지 못함");
        }
    }
    // 윈도우·리눅스는 따로 돌린다. 리눅스는 실제로 잠겼는지 확인하느라 몇 초 걸릴 수 있어서,
    // 부른 쪽(주 스레드의 메뉴 처리나 판단 스레드)이 그동안 멈추지 않게 한다.
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        if st.locking.swap(true, Ordering::SeqCst) {
            logging::info("이미 잠그는 중이라 건너뜀");
            return;
        }
        let st2 = st.clone();
        std::thread::spawn(move || {
            run();
            st2.locking.store(false, Ordering::SeqCst);
        });
    }
}

/// 사용자가 메뉴에서 바로 잠글 때
pub fn lock_now(app: &AppHandle) {
    let now = logic_now(&state(app));
    do_lock(app, "사용자가 '지금 잠그기' 선택", now);
}

/// 메뉴·설정 창에서 무언가 바꾼 뒤 판단 스레드에 바로 한 번 돌라고 알린다.
/// 판단을 여러 스레드에서 겹쳐 돌리면 늦게 모은 값이 나중에 반영되어 시간이 거꾸로 갈 수 있어서다.
pub fn tick_soon(app: &AppHandle) {
    let st = state(app);
    if st.manual.load(Ordering::SeqCst) {
        ui::schedule_refresh(app);
        return;
    }
    let (m, cv) = &st.wake;
    *lock(m) = true;
    cv.notify_one();
}
