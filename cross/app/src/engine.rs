//! 1초마다 시스템 상태를 읽어 잠금 여부를 판단하고 실행한다.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use wifilock_core::{EngineState, LockScheduler, SystemInputs, TickResult};

use crate::logging;
use crate::state::{lock, state, AppState};
use crate::ui;
use crate::views;

/// 와이파이 상태를 다시 읽는 간격. 바뀌면 순간 끊김 대비(15초)를 거치므로 몇 초 늦어도 판단에 영향이 없다.
/// (메뉴·설정 창에서 바로 다시 판단할 때는 즉시 읽는다)
const WIFI_EVERY: Duration = Duration::from_secs(5);
/// 위치 권한 상태를 다시 읽는 간격: 이미 허용됐으면 드물게, 아니면 허용하는 순간을 빨리 알아채도록 자주.
const LOCATION_EVERY_GRANTED: Duration = Duration::from_secs(30);
const LOCATION_EVERY_PENDING: Duration = Duration::from_secs(5);
/// 화면 유지 요청 목록을 다시 읽는 간격 (잠금이 가까워지면 매 초 읽는다)
const HOLDERS_EVERY: Duration = Duration::from_secs(5);
/// 운영체제가 스스로 잠그는 설정을 다시 읽는 간격 (설정 창이 열려 있는 동안만 읽는다)
const SYSTEM_LOCK_EVERY: Duration = Duration::from_secs(30);

/// 잠금이 가까운지 (경고 시작 10초 전부터). 이때는 화면 유지 요청을 매 초 확인하고 경고 창을 미리 준비한다.
pub fn near_lock(st: &AppState) -> bool {
    let warning = lock(&st.store).settings().warning_seconds as f64;
    let state = lock(&st.controller).status().state.clone();
    match state {
        EngineState::Counting { remaining, timeout } => remaining <= LockScheduler::effective_warning(timeout, warning) + 10.0,
        EngineState::Warning { .. } => true,
        _ => false,
    }
}

/// 판단 반복을 별도 스레드에서 시작한다.
pub fn start(app: AppHandle) {
    std::thread::Builder::new()
        .name("wifilock-engine".into())
        .spawn(move || {
            let mut last_kind = String::new();
            let mut forced = true;
            let mut last_wifi: Option<Instant> = None;
            let mut last_location: Option<Instant> = None;
            loop {
                let started = Instant::now();
                let st = state(&app);
                if !st.manual.load(Ordering::SeqCst) {
                    if forced || last_wifi.is_none_or(|t| t.elapsed() >= WIFI_EVERY) {
                        refresh_wifi(&st);
                        last_wifi = Some(Instant::now());
                    }
                    let every = if lock(&st.location).has_permission() { LOCATION_EVERY_GRANTED } else { LOCATION_EVERY_PENDING };
                    if forced || last_location.is_none_or(|t| t.elapsed() >= every) {
                        refresh_location(&st);
                        last_location = Some(Instant::now());
                    }
                    if app.get_webview_window(views::SETTINGS).is_some() {
                        refresh_system_locks(&st, false);
                    }
                    let r = tick(&app);
                    let kind = state_kind(&r.output.state);
                    if kind != last_kind {
                        logging::info(format!("상태: {}", wifilock_core::format::state_text(&r.output.state)));
                        last_kind = kind;
                    }
                }
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

/// 운영체제가 스스로 잠그는 설정을 필요하면 다시 읽는다 (설정 창 안내용).
/// 외부 명령을 부를 수 있으므로 주 스레드에서 부르지 않는다. 자가 진단 중(가짜 값 사용)에는 읽지 않는다.
pub fn refresh_system_locks(st: &AppState, force: bool) {
    if lock(&st.fake).is_some() {
        return;
    }
    if !force && lock(&st.system_locks).0.is_some_and(|t| t.elapsed() < SYSTEM_LOCK_EVERY) {
        return;
    }
    let list = st.platform.system_auto_locks();
    let mut g = lock(&st.system_locks);
    if g.0.is_none() || g.1 != list {
        let text: Vec<String> =
            list.iter().map(|l| format!("{} ({})", wifilock_core::format::duration(l.after_seconds), l.cause)).collect();
        logging::info(format!("운영체제 자체 잠금: {}", if text.is_empty() { "없음 또는 알 수 없음".into() } else { text.join(" / ") }));
    }
    *g = (Some(Instant::now()), list);
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
    let holders = if respect {
        // 전원 관리 쪽 질의는 다른 시스템 서비스의 일도 늘리므로 평소에는 5초마다, 잠금이 가까우면 매 초 읽는다.
        let due = lock(&st.holders).0.is_none_or(|t| t.elapsed() >= HOLDERS_EVERY) || near_lock(st);
        if due {
            let list = st.platform.display_sleep_preventers();
            *lock(&st.holders) = (Some(Instant::now()), list);
        }
        lock(&st.holders).1.clone()
    } else {
        Vec::new()
    };
    let clocks = wifilock_platform::clocks();
    let idle_seconds = if st.opts.simulate_away {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        Some(START.get_or_init(Instant::now).elapsed().as_secs_f64())
    } else {
        st.platform.idle_seconds()
    };
    SystemInputs {
        now: clocks.total,
        awake: clocks.awake,
        wall: wifilock_core::now(),
        idle_seconds,
        // 측정용 자리비움 흉내 중에는 화면이 잠기지 않은 것으로 본다 (남은 시간이 계속 줄어들게)
        screen_locked: !st.opts.simulate_away && st.platform.screen_locked(),
        assertion_holders: holders,
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
