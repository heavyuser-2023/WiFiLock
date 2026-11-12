use wifilock_core::{AppSettings, SettingsStore, TimeoutPresets, WiFiSnapshot, WiFiStatus};

fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("settings.json");
    (dir, path)
}

#[test]
fn defaults() {
    let (_d, path) = temp_store();
    let s = SettingsStore::open(&path).settings().clone();
    assert_eq!(s, AppSettings::default());
    assert!(s.enabled);
    assert_eq!(s.unknown_wifi_timeout_seconds, 300);
    assert_eq!(s.no_wifi_timeout_seconds, 180);
    assert!(s.rules.is_empty());
}

#[test]
fn persists_across_instances() {
    let (_d, path) = temp_store();
    let mut a = SettingsStore::open(&path);
    a.update(|s| {
        s.no_wifi_timeout_seconds = 60;
        s.warn_before_lock = false;
    });
    a.add_rule_for(&WiFiSnapshot::connected(Some("Home"), Some("1:2:3:4:5:6")), Some(3600));
    let b = SettingsStore::open(&path);
    assert_eq!(b.settings(), a.settings());
    assert_eq!(b.settings().rules[0].ssid.as_deref(), Some("Home"));
    assert_eq!(b.settings().rules[0].timeout_seconds, 3600);
    assert!(!path.with_extension("json.tmp").exists(), "임시 파일이 남지 않는다");
}

#[test]
fn corrupt_data_falls_back_to_defaults() {
    let (_d, path) = temp_store();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"not json").unwrap();
    assert_eq!(SettingsStore::open(&path).settings(), &AppSettings::default());
}

#[test]
fn partial_data_keeps_known_fields() {
    let (_d, path) = temp_store();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(
        &path,
        r#"{"noWiFiTimeoutSeconds": 45, "rules": [{"ssid": "X", "timeoutSeconds": 99}, 7], "futureField": 1, "enabled": "yes"}"#,
    )
    .unwrap();
    let s = SettingsStore::open(&path).settings().clone();
    assert_eq!(s.no_wifi_timeout_seconds, 45);
    assert_eq!(s.unknown_wifi_timeout_seconds, 300);
    assert!(s.enabled, "형식이 틀린 값은 기본값");
    assert_eq!(s.rules.len(), 1, "규칙이 아닌 값은 건너뛴다");
    assert_eq!(s.rules[0].name, "X", "이름이 없으면 와이파이 이름을 쓴다");
    assert_eq!(s.rules[0].timeout_seconds, 99);
}

#[test]
fn reads_settings_written_by_swift_app() {
    // 기존 맥 앱(스위프트)이 저장하던 형식과 같은 키를 쓴다.
    let json = r#"{"enabled":true,"rules":[{"id":"6F9619FF-8B86-D011-B42D-00C04FC964FF","name":"우리집","ssid":"Home","routerMAC":"d8:07:b6:47:6b:8c","timeoutSeconds":1800}],
        "unknownWiFiTimeoutSeconds":300,"noWiFiTimeoutSeconds":180,"warnBeforeLock":true,"warningSeconds":10,
        "respectDisplaySleepAssertions":true,"showCountdownInMenuBar":false}"#;
    let s: AppSettings = serde_json::from_str(json).unwrap();
    assert_eq!(s.rules[0].name, "우리집");
    assert_eq!(s.rules[0].id.to_string(), "6f9619ff-8b86-d011-b42d-00c04fc964ff");
    let back = serde_json::to_value(&s).unwrap();
    assert_eq!(back["rules"][0]["routerMAC"], "d8:07:b6:47:6b:8c");
    assert_eq!(back["unknownWiFiTimeoutSeconds"], 300);
    assert!(!s.countdown_in_minutes, "예전 설정 파일에는 없는 값이라 기본값(초 단위)");
}

#[test]
fn countdown_in_minutes_round_trips() {
    let s = AppSettings { show_countdown_in_menu_bar: true, countdown_in_minutes: true, ..AppSettings::default() };
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["showCountdownInMenuBar"], true);
    assert_eq!(v["countdownInMinutes"], true);
    let back: AppSettings = serde_json::from_value(v).unwrap();
    assert_eq!(back, s);
}

#[test]
fn add_rule_is_idempotent_and_fills_missing_ssid() {
    let (_d, path) = temp_store();
    let mut store = SettingsStore::open(&path);
    let r1 = store.add_rule_for(&WiFiSnapshot::connected(None, Some("aa:bb:cc:dd:ee:ff")), None).unwrap();
    assert!(r1.ssid.is_none());
    store.add_rule_for(&WiFiSnapshot::connected(None, Some("AA:BB:CC:DD:EE:FF")), None);
    assert_eq!(store.settings().rules.len(), 1);
    let r2 = store.add_rule_for(&WiFiSnapshot::connected(Some("Cafe"), Some("aa:bb:cc:dd:ee:ff")), None).unwrap();
    assert_eq!(store.settings().rules.len(), 1);
    assert_eq!(r2.ssid.as_deref(), Some("Cafe"));
    assert_eq!(r2.id, r1.id);
}

#[test]
fn same_name_other_router_gets_suffix() {
    let (_d, path) = temp_store();
    let mut store = SettingsStore::open(&path);
    store.add_rule_for(&WiFiSnapshot::connected(Some("iptime"), Some("02:00:00:00:00:01")), None);
    let r = store.add_rule_for(&WiFiSnapshot::connected(Some("iptime"), Some("02:00:00:00:aa:bb")), None).unwrap();
    assert_eq!(store.settings().rules.len(), 2);
    assert_eq!(r.name, "iptime (공유기 …aa:bb)");
}

#[test]
fn add_rule_by_ssid() {
    let (_d, path) = temp_store();
    let mut store = SettingsStore::open(&path);
    assert!(store.add_rule_ssid("  Office  ", 600).is_some());
    assert_eq!(store.settings().rules[0].ssid.as_deref(), Some("Office"));
    assert!(store.add_rule_ssid("Office", 60).is_none(), "중복은 추가하지 않는다");
    assert!(store.add_rule_ssid("   ", 60).is_none());
    assert_eq!(store.settings().rules.len(), 1);
    let id = store.settings().rules[0].id;
    assert!(store.remove_rule(id));
    assert!(store.settings().rules.is_empty());
    assert!(SettingsStore::open(&path).settings().rules.is_empty());
}

#[test]
fn move_rule_changes_order() {
    let mut store = SettingsStore::in_memory(AppSettings::default());
    store.add_rule_ssid("A", 60);
    store.add_rule_ssid("B", 60);
    let b = store.settings().rules[1].id;
    assert!(store.move_rule(b, true));
    assert_eq!(store.settings().rules[0].name, "B");
    assert!(!store.move_rule(b, true), "맨 위에서 더 올릴 수 없다");
}

#[test]
fn add_rule_when_disconnected_fails() {
    let mut store = SettingsStore::in_memory(AppSettings::default());
    assert!(store.add_rule_for(&WiFiSnapshot::status(WiFiStatus::Disconnected), None).is_none());
    assert!(store.add_rule_for(&WiFiSnapshot::connected(None, None), None).is_none());
    assert!(store.settings().rules.is_empty());
}

#[test]
fn value_clamping() {
    let s = AppSettings {
        unknown_wifi_timeout_seconds: -5,
        no_wifi_timeout_seconds: -1,
        warning_seconds: 0,
        ..AppSettings::default()
    }
    .clamped();
    assert_eq!(s.unknown_wifi_timeout_seconds, 0);
    assert_eq!(s.no_wifi_timeout_seconds, 0);
    assert_eq!(s.warning_seconds, 1);
}

#[test]
fn update_reports_change_only_when_different() {
    let mut store = SettingsStore::in_memory(AppSettings::default());
    assert!(!store.update(|_| {}));
    assert!(store.update(|s| s.enabled = false));
    assert!(!store.update(|s| s.enabled = false));
}

#[test]
fn timeout_presets_include_custom_value() {
    assert_eq!(TimeoutPresets::including(300), TimeoutPresets::ALL.to_vec());
    let custom = TimeoutPresets::including(45);
    assert!(custom.contains(&45));
    assert_eq!(*custom.last().unwrap(), 0, "'잠그지 않음' 은 항상 마지막");
    let head = &custom[..custom.len() - 1];
    let mut sorted = head.to_vec();
    sorted.sort();
    assert_eq!(head, &sorted[..]);
    assert_eq!(TimeoutPresets::warning_including(7), vec![5, 7, 10, 15, 30, 60]);
}
