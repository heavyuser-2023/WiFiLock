use wifilock_core::{EngineState, LockPolicy, LockScheduler, PolicySource, TickInput, TickOutput};

const T0: f64 = 1_000_000.0;

fn five_min() -> LockPolicy {
    LockPolicy::new(300, PolicySource::UnknownWiFi)
}

fn input(now: f64, idle: f64, policy: &LockPolicy, warn: bool, warning: f64) -> TickInput {
    let mut i = TickInput::basic(now, idle, policy.clone());
    i.warn_before_lock = warn;
    i.warning_seconds = warning;
    i
}

/// 자리비움 시간이 1초씩 늘어나는 상황을 흉내 낸다.
fn run(s: &mut LockScheduler, from: i64, to: i64, policy: &LockPolicy, warn: bool, warning: f64, start: f64) -> Vec<TickOutput> {
    (from..=to)
        .enumerate()
        .map(|(i, idle)| s.tick(&input(start + i as f64, idle as f64, policy, warn, warning)))
        .collect()
}

fn lock_count(v: &[TickOutput]) -> usize {
    v.iter().filter(|o| o.should_lock).count()
}

fn first_lock(v: &[TickOutput]) -> Option<usize> {
    v.iter().position(|o| o.should_lock)
}

#[test]
fn counts_down_and_locks_exactly_once_without_warning() {
    let mut s = LockScheduler::new();
    let outs = run(&mut s, 0, 400, &five_min(), false, 10.0, T0);
    assert_eq!(lock_count(&outs), 1);
    assert_eq!(first_lock(&outs), Some(300), "자리비움 300초에 잠가야 한다");
    assert_eq!(outs[10].state, EngineState::Counting { remaining: 290.0, timeout: 300.0 });
    assert_eq!(outs[350].state, EngineState::WaitingForActivity);
}

#[test]
fn warning_phase_then_lock() {
    let mut s = LockScheduler::new();
    let outs = run(&mut s, 0, 330, &five_min(), true, 10.0, T0);
    assert_eq!(lock_count(&outs), 1);
    assert_eq!(outs[289].state, EngineState::Counting { remaining: 11.0, timeout: 300.0 });
    assert_eq!(outs[290].warning_remaining(), Some(10.0));
    assert_eq!(outs[295].warning_remaining(), Some(5.0));
    assert_eq!(first_lock(&outs), Some(300));
}

#[test]
fn activity_during_warning_cancels_it() {
    let mut s = LockScheduler::new();
    run(&mut s, 0, 295, &five_min(), true, 10.0, T0);
    let after = s.tick(&TickInput::basic(T0 + 296.0, 0.2, five_min()));
    assert_eq!(after.state, EngineState::Counting { remaining: 299.8, timeout: 300.0 });
    assert!(!after.should_lock);
}

#[test]
fn rearms_after_user_returns() {
    let mut s = LockScheduler::new();
    assert_eq!(lock_count(&run(&mut s, 0, 310, &five_min(), false, 10.0, T0)), 1);
    assert_eq!(lock_count(&run(&mut s, 0, 310, &five_min(), false, 10.0, T0 + 1000.0)), 1);
}

#[test]
fn policy_shrink_while_idle_warns_first() {
    // 30분 규칙 네트워크에서 10분째 자리비움 중 와이파이가 끊겨 1분 규칙으로 바뀐 경우
    let mut s = LockScheduler::new();
    let long = LockPolicy::new(1800, PolicySource::UnknownWiFi);
    let short = LockPolicy::new(60, PolicySource::NoWiFi);
    run(&mut s, 590, 600, &long, true, 10.0, T0);
    let a = s.tick(&input(T0 + 11.0, 601.0, &short, true, 10.0));
    assert_eq!(a.warning_remaining(), Some(10.0), "바로 잠그지 않고 경고를 먼저 띄운다");
    assert!(!a.should_lock);
    let mut locked = false;
    for i in 1..=10 {
        let o = s.tick(&input(T0 + 11.0 + i as f64, 601.0 + i as f64, &short, true, 10.0));
        if o.should_lock {
            locked = true;
            assert_eq!(i, 10);
        }
    }
    assert!(locked);
}

#[test]
fn policy_shrink_without_warning_locks_immediately() {
    let mut s = LockScheduler::new();
    let short = LockPolicy::new(60, PolicySource::NoWiFi);
    assert!(s.tick(&input(T0, 601.0, &short, false, 10.0)).should_lock);
}

#[test]
fn never_lock_policy() {
    let mut s = LockScheduler::new();
    let never = LockPolicy::new(0, PolicySource::UnknownWiFi);
    let outs = run(&mut s, 0, 5000, &never, true, 10.0, T0);
    assert!(outs.iter().all(|o| !o.should_lock && o.state == EngineState::NeverLock));
}

#[test]
fn disabled() {
    let mut s = LockScheduler::new();
    let mut i = TickInput::basic(T0, 9999.0, five_min());
    i.enabled = false;
    let o = s.tick(&i);
    assert_eq!(o.state, EngineState::Disabled);
    assert!(!o.should_lock);
}

#[test]
fn paused_until_and_expiry() {
    let mut s = LockScheduler::new();
    let until = T0 + 60.0;
    let mut i = input(T0, 9999.0, &five_min(), false, 10.0);
    i.pause = Some(Some(until));
    let p = s.tick(&i);
    assert_eq!(p.state, EngineState::Paused { until: Some(until) });
    assert!(!p.should_lock);
    let mut j = input(T0 + 61.0, 10060.0, &five_min(), false, 10.0);
    j.pause = Some(Some(until));
    assert!(s.tick(&j).should_lock, "일시 중지가 끝나면 (여전히 자리비움이 길면) 잠근다");
}

#[test]
fn paused_indefinitely() {
    let mut s = LockScheduler::new();
    let mut i = TickInput::basic(T0, 9999.0, five_min());
    i.pause = Some(None);
    let p = s.tick(&i);
    assert_eq!(p.state, EngineState::Paused { until: None });
    assert!(!p.should_lock);
}

#[test]
fn assertion_holds_lock() {
    let mut s = LockScheduler::new();
    let mut i = TickInput::basic(T0, 9999.0, five_min());
    i.assertion_holders = vec!["동영상 재생기".into()];
    let o = s.tick(&i);
    assert_eq!(o.state, EngineState::HeldByAssertion { holders: vec!["동영상 재생기".into()] });
    assert!(!o.should_lock);
    let w = s.tick(&TickInput::basic(T0 + 1.0, 10000.0, five_min()));
    assert!(w.warning_remaining().is_some(), "재생이 끝나 요청이 사라지면 경고 후 잠금");
}

#[test]
fn screen_already_locked_does_not_lock_again() {
    let mut s = LockScheduler::new();
    for i in 0..100 {
        let mut inp = TickInput::basic(T0 + i as f64, 500.0 + i as f64, five_min());
        inp.screen_is_locked = true;
        let o = s.tick(&inp);
        assert_eq!(o.state, EngineState::ScreenLocked);
        assert!(!o.should_lock);
    }
    // 잠금이 풀렸지만 아직 입력이 없으면 다시 잠그지 않는다
    let o = s.tick(&TickInput::basic(T0 + 100.0, 600.0, five_min()));
    assert_eq!(o.state, EngineState::WaitingForActivity);
    // 입력이 생기면 다시 무장
    let r = s.tick(&TickInput::basic(T0 + 101.0, 0.1, five_min()));
    assert!(matches!(r.state, EngineState::Counting { .. }), "다시 세기 시작해야 한다: {:?}", r.state);
}

#[test]
fn note_user_activity_rearms() {
    let mut s = LockScheduler::new();
    run(&mut s, 0, 310, &five_min(), false, 10.0, T0);
    s.note_user_activity();
    assert!(s.tick(&input(T0 + 400.0, 311.0, &five_min(), false, 10.0)).should_lock);
}

#[test]
fn timeout_shorter_than_warning() {
    let mut s = LockScheduler::new();
    let p = LockPolicy::new(5, PolicySource::NoWiFi);
    let outs = run(&mut s, 0, 20, &p, true, 10.0, T0);
    // 경고는 잠금 시간의 절반(2.5초)으로 줄어든다: 3초에 경고 시작, 6초쯤 잠금
    assert!(outs[0].warning_remaining().is_none(), "자리비움 0초에서는 경고하지 않는다");
    assert_eq!(outs.iter().position(|o| o.warning_remaining().is_some()), Some(3));
    assert_eq!(first_lock(&outs), Some(6));
    assert_eq!(lock_count(&outs), 1);
}

/// 검토에서 찾은 결함: 경고 시간이 잠금 시간 이상이면 계속 입력 중인데도 잠갔다.
#[test]
fn continuous_activity_never_locks_even_if_warning_exceeds_timeout() {
    for (timeout, warning) in [(30, 30.0), (30, 60.0), (60, 60.0), (5, 10.0)] {
        let mut s = LockScheduler::new();
        let p = LockPolicy::new(timeout, PolicySource::UnknownWiFi);
        for i in 0..300 {
            let idle = (i % 4) as f64 * 0.1;
            let o = s.tick(&input(T0 + i as f64, idle, &p, true, warning));
            assert!(!o.should_lock, "잠금 {timeout}초 / 경고 {warning}초, {i}초째");
            assert!(o.warning_remaining().is_none(), "잠금 {timeout}초 / 경고 {warning}초, {i}초째");
        }
    }
}

#[test]
fn slow_typing_just_under_one_second_does_not_lock() {
    let mut s = LockScheduler::new();
    let p = LockPolicy::new(30, PolicySource::UnknownWiFi);
    for i in 0..200 {
        let idle = [0.6, 0.9, 0.7, 0.95][i % 4];
        assert!(!s.tick(&input(T0 + i as f64, idle, &p, true, 30.0)).should_lock);
    }
}

#[test]
fn effective_warning() {
    assert_eq!(LockScheduler::effective_warning(300.0, 10.0), 10.0);
    assert_eq!(LockScheduler::effective_warning(30.0, 30.0), 15.0);
    assert_eq!(LockScheduler::effective_warning(30.0, 60.0), 15.0);
    assert_eq!(LockScheduler::effective_warning(0.0, 10.0), 0.0);
}
