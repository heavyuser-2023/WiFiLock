//! 가짜 시간·가짜 와이파이로 제어 흐름 전체를 돌리는 시나리오 (기존 맥 앱 자가 진단과 같은 시나리오)

use wifilock_core::{
    AppSettings, Controller, EngineState, PolicySource, SettingsStore, SystemInputs, TickResult, WiFiSnapshot, WiFiStatus,
};

struct Sim {
    c: Controller,
    store: SettingsStore,
    now: f64,
    idle: f64,
    locked: bool,
    holders: Vec<String>,
    wifi: WiFiSnapshot,
    locks: usize,
}

impl Sim {
    fn new() -> Self {
        let mut store = SettingsStore::in_memory(AppSettings::default());
        store.update(|s| {
            s.unknown_wifi_timeout_seconds = 120;
            s.no_wifi_timeout_seconds = 60;
            s.warn_before_lock = true;
            s.warning_seconds = 10;
        });
        Sim {
            c: Controller::new(None),
            store,
            now: 1_700_000_000.0,
            idle: 0.0,
            locked: false,
            holders: vec![],
            wifi: WiFiSnapshot::unknown(),
            locks: 0,
        }
    }

    fn tick(&mut self) -> TickResult {
        let r = self.c.tick(
            self.store.settings(),
            SystemInputs {
                now: self.now,
                idle_seconds: Some(self.idle),
                screen_locked: self.locked,
                assertion_holders: self.holders.clone(),
                wifi: self.wifi.clone(),
            },
        );
        if r.lock_reason.is_some() {
            self.locks += 1;
            self.c.did_lock(self.now);
        }
        r
    }

    fn advance(&mut self, seconds: usize) -> Vec<TickResult> {
        (0..seconds)
            .map(|_| {
                self.now += 1.0;
                self.idle += 1.0;
                self.tick()
            })
            .collect()
    }
}

fn first_warn(v: &[TickResult]) -> Option<usize> {
    v.iter().position(|r| r.output.warning_remaining().is_some()).map(|i| i + 1)
}
fn first_lock(v: &[TickResult]) -> Option<usize> {
    v.iter().position(|r| r.lock_reason.is_some()).map(|i| i + 1)
}

fn home() -> WiFiSnapshot {
    WiFiSnapshot::new(WiFiStatus::Connected, Some("SelfTestHome"), Some("02:00:00:00:00:01"), Some("en0"))
}

#[test]
fn full_scenario_like_the_mac_self_test() {
    let mut s = Sim::new();
    s.store.add_rule_for(&home(), Some(1800));

    // 1) 집 와이파이: 30분 규칙
    s.wifi = home();
    s.tick();
    assert_eq!(s.c.status().policy.timeout_seconds, 1800);
    assert!(s.c.status().policy.source.is_rule());
    s.advance(100);
    assert_eq!(s.locks, 0, "집에서 100초 자리비움 → 잠그지 않음");

    // 1-1) 와이파이가 5초 끊겼다 다시 붙음 → 집 규칙 유지, 경고·잠금 없음
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
    let flap = s.advance(5);
    assert_eq!(s.c.status().policy.timeout_seconds, 1800);
    assert!(s.c.status().policy_change_pending);
    s.wifi = home();
    let after = s.advance(3);
    assert!(!flap.iter().chain(after.iter()).any(|r| r.lock_reason.is_some() || r.output.warning_remaining().is_some()));
    assert!(!s.c.status().policy_change_pending);
    assert_eq!(s.locks, 0);

    // 2) 끊김이 15초 이어짐 → 1분 규칙, 이미 넘었으므로 경고 10초 후 잠금
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
    let first = s.tick();
    assert_eq!(s.c.status().policy.timeout_seconds, 1800, "끊긴 직후엔 집 규칙 유지(확인 중)");
    assert!(s.c.status().policy_change_pending && first.lock_reason.is_none());
    let outs = s.advance(30);
    assert_eq!(s.c.status().policy.timeout_seconds, 60);
    assert_eq!(s.c.status().policy.source, PolicySource::NoWiFi);
    assert_eq!(first_warn(&outs), Some(15), "시간 초과 시 경고 먼저");
    assert_eq!(first_lock(&outs), Some(25), "경고 10초 뒤 잠금");
    assert_eq!(s.locks, 1);
    s.advance(120);
    assert_eq!(s.locks, 1, "같은 자리비움 동안 다시 잠그지 않음");

    // 2-1) 입력 없이 잠금이 풀림 → 곧바로 다시 잠그지 않고 해제 시각부터 다시 센다
    let before = s.locks;
    s.c.note_presence(s.now);
    let after_unlock = s.advance(45);
    assert_eq!(s.locks, before);
    assert!(!after_unlock.iter().any(|r| r.output.warning_remaining().is_some()));
    let rest = s.advance(20);
    assert_eq!(s.locks, before + 1, "해제 후 1분 지나면 다시 잠금");
    assert!(rest.iter().any(|r| r.lock_reason.is_some()));

    // 3) 사용자가 돌아옴 → 다시 1분 뒤 잠금
    s.idle = 0.0;
    s.tick();
    let outs2 = s.advance(70);
    assert_eq!(first_warn(&outs2), Some(50));
    assert_eq!(first_lock(&outs2), Some(60));
    assert_eq!(s.locks, 3);

    // 4) 등록되지 않은 와이파이 → 2분
    s.idle = 0.0;
    s.wifi = WiFiSnapshot::connected(Some("Cafe"), Some("02:00:00:00:00:99"));
    s.tick();
    let outs3 = s.advance(125);
    assert_eq!(s.c.status().policy.source, PolicySource::UnknownWiFi);
    assert_eq!(first_lock(&outs3), Some(120));
    assert_eq!(s.locks, 4);

    // 5) 이름을 모를 때 공유기 주소로 집을 알아봄
    s.idle = 0.0;
    s.wifi = WiFiSnapshot::connected(None, Some("02:00:00:00:00:01"));
    s.tick();
    assert_eq!(s.c.status().policy.timeout_seconds, 1800);

    // 6) 화면 유지 요청이 있으면 보류
    s.idle = 0.0;
    s.wifi = WiFiSnapshot::status(WiFiStatus::PoweredOff);
    s.holders = vec!["동영상 재생기".into()];
    let held = s.advance(90);
    assert_eq!(s.locks, 4);
    assert_eq!(held.last().unwrap().output.state, EngineState::HeldByAssertion { holders: vec!["동영상 재생기".into()] });
    s.holders.clear();
    let released = s.advance(11);
    assert!(released.iter().any(|r| r.output.warning_remaining().is_some()));
    assert_eq!(s.locks, 5, "재생 끝나면 경고 후 잠금");

    // 7) "잠그지 않음" 규칙
    s.idle = 0.0;
    s.store.update(|st| st.rules[0].timeout_seconds = 0);
    s.wifi = home();
    s.advance(4000);
    assert_eq!(s.locks, 5);
    assert_eq!(s.c.status().state, EngineState::NeverLock);

    // 8) 끄기
    s.store.update(|st| {
        st.rules[0].timeout_seconds = 30;
        st.enabled = false;
    });
    s.advance(100);
    assert_eq!(s.locks, 5);
    assert_eq!(s.c.status().state, EngineState::Disabled);

    // 9) 계속 사용 중: 잠금 30초 + 경고 60초처럼 경고가 더 길어도 잠그지 않음
    s.store.update(|st| {
        st.enabled = true;
        st.warning_seconds = 60;
    });
    for i in 0..120 {
        s.now += 1.0;
        s.idle = (i % 4) as f64 * 0.1;
        let r = s.tick();
        assert!(r.lock_reason.is_none() && r.output.warning_remaining().is_none(), "{i}초째");
    }
    assert_eq!(s.locks, 5);
}

#[test]
fn unlock_transition_counts_as_presence() {
    let mut s = Sim::new();
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected); // 60초 규칙
    s.idle = 500.0;
    s.locked = true;
    s.advance(10);
    assert_eq!(s.c.status().state, EngineState::ScreenLocked);
    // 얼굴 인식·시계 등으로 입력 없이 풀림: 자리비움 시간은 여전히 크다
    s.locked = false;
    let outs = s.advance(70);
    // 풀린 것을 알아챈 순간이 0초: 50초 지나 경고(51번째), 60초 지나 잠금(61번째)
    assert_eq!(first_warn(&outs), Some(51), "풀린 시각부터 다시 센다");
    assert_eq!(first_lock(&outs), Some(61));
}

#[test]
fn long_gap_between_ticks_counts_as_wake() {
    let mut s = Sim::new();
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected); // 60초 규칙
    s.tick();
    // 컴퓨터가 잠자기에 들어가 2시간 동안 아무것도 돌지 않음
    s.now += 7200.0;
    s.idle += 7200.0;
    let r = s.tick();
    assert!(r.lock_reason.is_none() && r.output.warning_remaining().is_none(), "깨어나자마자 잠그지 않는다");
    let outs = s.advance(70);
    assert_eq!(first_lock(&outs), Some(60));
}

#[test]
fn pause_and_resume() {
    let mut s = Sim::new();
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
    let now = s.now;
    s.c.pause_for(now, Some(1800.0));
    s.idle = 5000.0;
    s.tick();
    assert!(matches!(s.c.status().state, EngineState::Paused { until: Some(_) }));
    assert!(s.c.status().paused);
    // 30분이 지나면 일시 중지가 저절로 끝나고, 자리비움이 길면 경고 10초 뒤 잠근다
    let outs = s.advance(1811);
    assert!(!s.c.is_paused());
    assert_eq!(first_warn(&outs), Some(1800));
    assert_eq!(first_lock(&outs), Some(1810));
    // 무기한 일시 중지 → 다시 시작
    s.idle = 0.0;
    let now = s.now;
    s.c.pause_for(now, None);
    s.advance(300);
    assert_eq!(s.c.status().state, EngineState::Paused { until: None });
    s.c.resume();
    s.tick();
    assert!(!s.c.status().paused);
}

#[test]
fn idle_unavailable_never_locks_but_shows_other_states() {
    let mut s = Sim::new();
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
    let r = s.c.tick(
        s.store.settings(),
        SystemInputs { now: s.now, idle_seconds: None, screen_locked: false, assertion_holders: vec![], wifi: s.wifi.clone() },
    );
    assert_eq!(r.output.state, EngineState::IdleUnavailable);
    assert!(r.lock_reason.is_none());
    s.store.update(|st| st.enabled = false);
    let r = s.c.tick(
        s.store.settings(),
        SystemInputs { now: s.now + 1.0, idle_seconds: None, screen_locked: false, assertion_holders: vec![], wifi: s.wifi.clone() },
    );
    assert_eq!(r.output.state, EngineState::Disabled);
}

#[test]
fn respect_assertions_setting_off_ignores_holders() {
    let mut s = Sim::new();
    s.store.update(|st| st.respect_display_sleep_assertions = false);
    s.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
    s.holders = vec!["재생기".into()];
    let outs = s.advance(80);
    assert_eq!(first_lock(&outs), Some(60));
}

#[test]
fn override_timeout_applies_everywhere() {
    let mut s = Sim::new();
    s.c = Controller::new(Some(5));
    s.wifi = WiFiSnapshot::connected(Some("Any"), None);
    s.tick();
    assert_eq!(s.c.status().policy.timeout_seconds, 5);
}

#[test]
fn status_changed_flag() {
    let mut s = Sim::new();
    let a = s.tick();
    assert!(a.changed);
    s.idle = 0.0;
    let b = s.tick();
    let c = s.tick();
    assert!(b.changed || !c.changed, "같은 상태가 이어지면 바뀌지 않음");
    assert!(!c.changed);
}
