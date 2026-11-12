use wifilock_core::{
    mac_normalize, AppSettings, LockPolicy, NetworkRule, PolicyResolver, PolicySource, PolicyStabilizer, WiFiSnapshot,
    WiFiStatus,
};

fn home() -> NetworkRule {
    NetworkRule::new("우리집", Some("Home_5G"), Some("d8:7:b6:47:6b:8c"), 1800)
}
fn office() -> NetworkRule {
    NetworkRule::new("사무실", Some("Office"), None, 600)
}
fn cafe_by_mac() -> NetworkRule {
    NetworkRule::new("카페", None, Some("aa:bb:cc:dd:ee:ff"), 0)
}

struct Fixture {
    home: NetworkRule,
    office: NetworkRule,
    cafe: NetworkRule,
    settings: AppSettings,
}

fn fixture() -> Fixture {
    let (h, o, c) = (home(), office(), cafe_by_mac());
    let settings = AppSettings {
        rules: vec![h.clone(), o.clone(), c.clone()],
        unknown_wifi_timeout_seconds: 120,
        no_wifi_timeout_seconds: 60,
        ..AppSettings::default()
    };
    Fixture { home: h, office: o, cafe: c, settings }
}

fn rule_source(r: &NetworkRule) -> PolicySource {
    PolicySource::Rule { rule: r.clone() }
}

#[test]
fn mac_normalization() {
    assert_eq!(mac_normalize("d8:7:b6:47:6b:8c").as_deref(), Some("d8:07:b6:47:6b:8c"));
    assert_eq!(mac_normalize(" AA:BB:CC:DD:EE:FF\n").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!(mac_normalize("AA-BB-CC-DD-EE-FF").as_deref(), Some("aa:bb:cc:dd:ee:ff"), "윈도우식 표기");
    assert_eq!(mac_normalize("(incomplete)"), None);
    assert_eq!(mac_normalize("aa:bb:cc:dd:ee"), None);
    assert_eq!(mac_normalize("aa:bb:cc:dd:ee:zz"), None);
    assert_eq!(mac_normalize("0:0:0:0:0:0"), None);
    assert_eq!(mac_normalize("ff:ff:ff:ff:ff:ff"), None);
    assert_eq!(mac_normalize("aaa:bb:cc:dd:ee:ff"), None);
    assert_eq!(mac_normalize("aa::cc:dd:ee:ff"), None);
}

#[test]
fn rule_stores_normalized_mac() {
    assert_eq!(home().router_mac.as_deref(), Some("d8:07:b6:47:6b:8c"));
}

#[test]
fn no_wifi_uses_no_wifi_timeout() {
    let f = fixture();
    for status in [WiFiStatus::PoweredOff, WiFiStatus::Disconnected, WiFiStatus::Unavailable] {
        let p = PolicyResolver::resolve(&WiFiSnapshot::status(status), &f.settings);
        assert_eq!(p, LockPolicy::new(60, PolicySource::NoWiFi), "{status:?}");
    }
}

#[test]
fn match_by_ssid() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Office"), None), &f.settings);
    assert_eq!(p.timeout_seconds, 600);
    assert_eq!(p.source, rule_source(&f.office));
}

#[test]
fn ssid_is_case_sensitive_and_exact() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("office"), None), &f.settings);
    assert_eq!(p.source, PolicySource::UnknownWiFi);
    assert_eq!(p.timeout_seconds, 120);
}

#[test]
fn unknown_ssid_falls_back_to_unknown_wifi() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Starbucks"), Some("11:22:33:44:55:66")), &f.settings);
    assert_eq!(p, LockPolicy::new(120, PolicySource::UnknownWiFi));
}

#[test]
fn match_by_router_when_ssid_unavailable() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(None, Some("D8:07:B6:47:6B:8C")), &f.settings);
    assert_eq!(p.source, rule_source(&f.home));
    assert_eq!(p.timeout_seconds, 1800);
}

#[test]
fn router_match_does_not_override_different_known_ssid() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Home_Guest"), Some("d8:07:b6:47:6b:8c")), &f.settings);
    assert_eq!(p.source, PolicySource::UnknownWiFi);
}

/// 검토에서 찾은 결함: 이름만 같은 다른 공유기의 네트워크를 집으로 착각했다.
#[test]
fn same_ssid_different_router_does_not_match() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Home_5G"), Some("99:99:99:99:99:99")), &f.settings);
    assert_eq!(p.source, PolicySource::UnknownWiFi);
}

#[test]
fn same_ssid_with_unknown_router_still_matches() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Home_5G"), None), &f.settings);
    assert_eq!(p.source, rule_source(&f.home));
    let q = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Office"), Some("99:99:99:99:99:99")), &f.settings);
    assert_eq!(q.source, rule_source(&f.office), "이름으로만 등록한 규칙은 어느 공유기든 맞는다");
}

#[test]
fn mac_only_rule_matches_even_when_ssid_known() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("CafeFree"), Some("aa:bb:cc:dd:ee:ff")), &f.settings);
    assert_eq!(p.source, rule_source(&f.cafe));
    assert!(p.never_locks());
}

#[test]
fn connected_without_any_identifier_is_unknown() {
    let f = fixture();
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(None, None), &f.settings);
    assert_eq!(p.source, PolicySource::UnknownWiFi);
}

#[test]
fn first_matching_rule_wins() {
    let mut f = fixture();
    f.settings.rules.push(NetworkRule::new("중복", Some("Office"), None, 30));
    let p = PolicyResolver::resolve(&WiFiSnapshot::connected(Some("Office"), None), &f.settings);
    assert_eq!(p.timeout_seconds, 600);
}

#[test]
fn make_rule() {
    assert!(PolicyResolver::make_rule(&WiFiSnapshot::status(WiFiStatus::Disconnected), None).is_none());
    assert!(PolicyResolver::make_rule(&WiFiSnapshot::connected(None, None), None).is_none());
    let by_name = PolicyResolver::make_rule(&WiFiSnapshot::connected(Some("A"), Some("1:2:3:4:5:6")), None).unwrap();
    assert_eq!(by_name.name, "A");
    assert_eq!(by_name.router_mac.as_deref(), Some("01:02:03:04:05:06"));
    assert_eq!(by_name.timeout_seconds, 1800);
    let by_mac = PolicyResolver::make_rule(&WiFiSnapshot::connected(None, Some("1:2:3:4:5:6")), Some(600)).unwrap();
    assert_eq!(by_mac.name, "공유기 01:02:03:04:05:06");
    assert!(by_mac.ssid.is_none());
    assert_eq!(by_mac.timeout_seconds, 600);
}

#[test]
fn snapshot_descriptions() {
    assert_eq!(WiFiSnapshot::status(WiFiStatus::PoweredOff).korean_description(), "와이파이 꺼짐");
    assert_eq!(WiFiSnapshot::connected(Some("Home"), None).korean_description(), "Home");
    assert_eq!(WiFiSnapshot::connected(None, None).korean_description(), "이름 확인 불가 (권한 필요)");
}

// ---- 순간 끊김 대비 (검토에서 찾은 결함: 2~3초 끊겨도 짧은 잠금 시간이 바로 적용됐다) ----

const T0: f64 = 2_000_000.0;

fn stab_settings() -> AppSettings {
    AppSettings {
        rules: vec![NetworkRule::new("Home", Some("Home"), Some("02:00:00:00:00:01"), 1800)],
        unknown_wifi_timeout_seconds: 300,
        no_wifi_timeout_seconds: 60,
        ..AppSettings::default()
    }
}
fn s_home() -> WiFiSnapshot {
    WiFiSnapshot::connected(Some("Home"), Some("02:00:00:00:00:01"))
}
fn s_cafe() -> WiFiSnapshot {
    WiFiSnapshot::connected(Some("Cafe"), Some("02:00:00:00:00:02"))
}
fn s_off() -> WiFiSnapshot {
    WiFiSnapshot::status(WiFiStatus::Disconnected)
}

#[test]
fn brief_drop_is_ignored() {
    let st = stab_settings();
    let mut s = PolicyStabilizer::new(15.0);
    assert_eq!(s.update(&s_home(), &st, T0), s_home());
    for i in 1..=5 {
        assert_eq!(s.update(&s_off(), &st, T0 + i as f64), s_home(), "{i}초째에는 아직 집 규칙");
    }
    assert!(s.pending_since().is_some());
    assert_eq!(s.update(&s_home(), &st, T0 + 6.0), s_home());
    assert!(s.pending_since().is_none());
}

#[test]
fn sustained_drop_is_applied_after_delay() {
    let st = stab_settings();
    let mut s = PolicyStabilizer::new(15.0);
    s.update(&s_home(), &st, T0);
    assert_eq!(s.update(&s_off(), &st, T0 + 1.0), s_home());
    assert_eq!(s.update(&s_off(), &st, T0 + 15.0), s_home());
    assert_eq!(s.update(&s_off(), &st, T0 + 16.0), s_off());
}

#[test]
fn flapping_between_two_stricter_states_keeps_timer_if_same_policy() {
    let st = stab_settings();
    let mut s = PolicyStabilizer::new(15.0);
    s.update(&s_home(), &st, T0);
    s.update(&s_off(), &st, T0 + 1.0);
    let powered = WiFiSnapshot::status(WiFiStatus::PoweredOff);
    s.update(&powered, &st, T0 + 8.0);
    assert_eq!(s.update(&powered, &st, T0 + 16.0), powered);
}

#[test]
fn longer_timeout_is_applied_immediately() {
    let st = stab_settings();
    let mut s = PolicyStabilizer::new(15.0);
    s.update(&s_off(), &st, T0);
    assert_eq!(s.update(&s_cafe(), &st, T0 + 1.0), s_cafe(), "60초 → 300초: 바로");
    assert_eq!(s.update(&s_home(), &st, T0 + 2.0), s_home(), "300초 → 1800초: 바로");
}

#[test]
fn switch_to_stricter_network_waits() {
    let st = stab_settings();
    let mut s = PolicyStabilizer::new(15.0);
    s.update(&s_home(), &st, T0);
    assert_eq!(s.update(&s_cafe(), &st, T0 + 1.0), s_home());
    assert_eq!(s.update(&s_cafe(), &st, T0 + 17.0), s_cafe());
}

#[test]
fn never_lock_ordering() {
    let never = LockPolicy::new(0, PolicySource::UnknownWiFi);
    let five = LockPolicy::new(300, PolicySource::UnknownWiFi);
    assert!(PolicyStabilizer::is_stricter(&five, &never));
    assert!(!PolicyStabilizer::is_stricter(&never, &five));
    assert!(!PolicyStabilizer::is_stricter(&five, &five));
}
