use wifilock_core::{
    AppSettings, BluetoothDeviceKind, BluetoothDeviceRule, BluetoothObservation, BluetoothProximityTracker, Controller,
    EngineState, Lang, LockPolicy, LockScheduler, PolicySource, ProximityRange, SettingsStore, SystemInputs, TickInput,
    WiFiSnapshot, WiFiStatus,
};

fn obs(key: &str, ble_id: Option<&str>, mac: Option<&str>, classic: Option<&str>, rssi: Option<i16>, connected: bool, seen_at: f64) -> BluetoothObservation {
    BluetoothObservation {
        key: key.into(),
        name: Some(key.into()),
        kind: BluetoothDeviceKind::Other,
        ble_id: ble_id.map(str::to_owned),
        mac_address: mac.map(str::to_owned),
        classic_address: classic.map(str::to_owned),
        rssi,
        connected,
        paired: classic.is_some(),
        seen_at,
    }
}

#[test]
fn three_range_presets_and_labels() {
    assert_eq!(ProximityRange::default(), ProximityRange::Near);
    assert_eq!(ProximityRange::ALL, [ProximityRange::Near, ProximityRange::Medium, ProximityRange::Far]);
    assert_eq!(ProximityRange::Near.label_in(Lang::Ko), "가까움 (약 1~2m)");
    assert_eq!(ProximityRange::Medium.label_in(Lang::Ko), "중간 가까움 (약 3~5m)");
    assert_eq!(ProximityRange::Far.label_in(Lang::Ko), "먼 거리 (약 6~10m)");
    assert_eq!(ProximityRange::Near.label_in(Lang::En), "Near (~1–2m)");
    assert_eq!(ProximityRange::Medium.label_in(Lang::En), "Medium (~3–5m)");
    assert_eq!(ProximityRange::Far.label_in(Lang::En), "Far (~6–10m)");

    assert!(ProximityRange::Near.enter_rssi() > ProximityRange::Medium.enter_rssi());
    assert!(ProximityRange::Medium.enter_rssi() > ProximityRange::Far.enter_rssi());
}

#[test]
fn settings_backward_compatibility_and_store_helpers() {
    let legacy = r#"{"enabled":true,"rules":[],"unknownWiFiTimeoutSeconds":300,"noWiFiTimeoutSeconds":180}"#;
    let parsed: AppSettings = serde_json::from_str(legacy).unwrap();
    assert!(!parsed.bluetooth_hold_enabled);
    assert_eq!(parsed.bluetooth_range, ProximityRange::Near);
    assert!(parsed.bluetooth_devices.is_empty());

    let mut store = SettingsStore::in_memory(AppSettings::default());
    let band = BluetoothDeviceRule::new(
        "Xiaomi Smart Band 10",
        BluetoothDeviceKind::Band,
        Some("7B7B8BF0-E1AF-EC94-3FB0-F656EF63A35E"),
        Some("04:34:C3:64:6F:D1"),
        None,
    );
    let saved = store.add_bluetooth_device(band).unwrap();
    assert!(store.settings().bluetooth_hold_enabled, "첫 기기 등록 시 자동 활성화");
    assert_eq!(saved.mac_address.as_deref(), Some("04:34:c3:64:6f:d1"));

    // 같은 MAC으로 다시 추가하면 중복 생성하지 않고 병합
    let dup = BluetoothDeviceRule::new(
        "Band Alias",
        BluetoothDeviceKind::Band,
        None,
        Some("04-34-C3-64-6F-D1"),
        None,
    );
    let merged = store.add_bluetooth_device(dup).unwrap();
    assert_eq!(store.settings().bluetooth_devices.len(), 1);
    assert_eq!(merged.id, saved.id);
    assert_eq!(merged.ble_id.as_deref(), Some("7B7B8BF0-E1AF-EC94-3FB0-F656EF63A35E"));

    assert!(store.remove_bluetooth_device(saved.id));
    assert!(store.settings().bluetooth_devices.is_empty());
}

#[test]
fn tracker_median_spike_filter_hysteresis_and_leave_grace() {
    let mut tracker = BluetoothProximityTracker::new();
    let band = BluetoothDeviceRule::new("미밴드", BluetoothDeviceKind::Band, None, Some("04:34:c3:64:6f:d1"), None);
    let rules = [band.clone()];

    // 1) Near 모드(-65 진입, -74 이탈): -58 dBm 들어오면 보류 시작
    let r1 = tracker.update(
        10.0,
        ProximityRange::Near,
        &rules,
        &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-58), false, 10.0)],
    );
    assert_eq!(r1.holding_devices, vec!["미밴드"]);

    // 2) 아이폰 실측처럼 1회성 -81 dBm 스파이크가 섞여도 중앙값으로 버팀
    for i in 1..=3 {
        let rssi = if i == 2 { -81 } else { -58 };
        let r = tracker.update(
            10.0 + i as f64,
            ProximityRange::Near,
            &rules,
            &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(rssi), false, 10.0 + i as f64)],
        );
        assert_eq!(r.holding_devices, vec!["미밴드"], "1회 스파이크는 중앙값으로 걸러짐");
        assert!(!r.any_leaving);
    }

    // 3) 이탈 구간(-78 dBm)이 계속 들어오면 15초 유예 동안 보류를 유지하다가 15초 후 해제
    for sec in 0..14 {
        let t = 35.0 + sec as f64;
        let r = tracker.update(
            t,
            ProximityRange::Near,
            &rules,
            &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-78), false, t)],
        );
        // 초반 몇 초는 이전 샘플과 섞여 중앙값이 아직 -74 이상일 수 있으나, 일단 이탈이 시작되면 15초 전엔 절대 안 풀림
        assert_eq!(r.holding_devices, vec!["미밴드"], "t={t} 이탈 유예 중 유지");
    }
    let mut released = false;
    for sec in 14..=25 {
        let t = 35.0 + sec as f64;
        let r = tracker.update(
            t,
            ProximityRange::Near,
            &rules,
            &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-78), false, t)],
        );
        if r.holding_devices.is_empty() {
            released = true;
            break;
        }
    }
    assert!(released, "15초 연속 범위 밖이면 보류 해제");
}

#[test]
fn tracker_medium_and_far_thresholds_and_or_logic() {
    let mut tracker = BluetoothProximityTracker::new();
    let phone = BluetoothDeviceRule::new("아이폰", BluetoothDeviceKind::Phone, Some("uuid-phone"), None, Some("64:6d:2f:a7:28:fc"));
    let airpods = BluetoothDeviceRule::new("에어팟", BluetoothDeviceKind::Audio, None, None, Some("cc:22:fe:38:f9:c4"));
    let rules = [phone, airpods];

    // -72 dBm은 Near(-65)에서는 못 들어가지만 Medium(-76)에서는 보류
    let r_near = tracker.update(10.0, ProximityRange::Near, &rules, &[obs("p", Some("uuid-phone"), None, None, Some(-72), false, 10.0)]);
    assert!(r_near.holding_devices.is_empty());

    let r_med1 = tracker.update(11.0, ProximityRange::Medium, &rules, &[obs("p", Some("uuid-phone"), None, None, Some(-72), false, 11.0)]);
    assert_eq!(r_med1.holding_devices, vec!["아이폰"]);

    // -84 dBm은 Medium(-76)에서는 안 되지만 Far(-88)에서는 보류 (새 기기 테스트)
    let mut t_far = BluetoothProximityTracker::new();
    let r_far1 = t_far.update(10.0, ProximityRange::Far, &rules, &[obs("p", Some("uuid-phone"), None, None, Some(-84), false, 10.0)]);
    assert_eq!(r_far1.holding_devices, vec!["아이폰"]);

    // 에어팟처럼 connected=true인데 RSSI=0(None)이어도 보류 폴백 동작
    let mut t_conn = BluetoothProximityTracker::new();
    let r_conn = t_conn.update(10.0, ProximityRange::Near, &rules, &[obs("a", None, None, Some("cc:22:fe:38:f9:c4"), None, true, 10.0)]);
    assert_eq!(r_conn.holding_devices, vec!["에어팟"]);

    // connected=true이면서 -67 dBm인 에어팟은 Near(enter=-65, exit=-74)에서도 진입 보류되고,
    // -80 dBm으로 약해져 15초 유예가 지나면 해제된다.
    let mut t_air = BluetoothProximityTracker::new();
    let r_enter = t_air.update(10.0, ProximityRange::Near, &rules, &[obs("a", None, None, Some("cc:22:fe:38:f9:c4"), Some(-67), true, 10.0)]);
    assert_eq!(r_enter.holding_devices, vec!["에어팟"]);

    let mut unlocked = false;
    for sec in 11..=55 {
        let t = sec as f64;
        let rep = t_air.update(t, ProximityRange::Near, &rules, &[obs("a", None, None, Some("cc:22:fe:38:f9:c4"), Some(-80), true, t)]);
        if rep.holding_devices.is_empty() {
            unlocked = true;
            break;
        }
    }
    assert!(unlocked, "연결된 기기도 이탈 문턱 이하로 멀어지면 유예 후 보류 해제");
}

#[test]
fn scheduler_and_controller_bluetooth_hold_flow() {
    let mut sched = LockScheduler::new();
    let mut ti = TickInput::basic(100.0, 500.0, LockPolicy::new(60, PolicySource::NoWiFi));
    ti.bluetooth_holders = vec!["내 아이폰".into()];
    let out = sched.tick(&ti);
    assert_eq!(out.state, EngineState::HeldByBluetooth { devices: vec!["내 아이폰".into()] });
    assert!(!out.should_lock);

    // 동영상 보류가 있으면 동영상이 우선
    ti.assertion_holders = vec!["VLC".into()];
    assert_eq!(sched.tick(&ti).state, EngineState::HeldByAssertion { holders: vec!["VLC".into()] });

    // Controller 통합: 보류 해제 후 경고 창을 띄운 뒤 잠근다
    let mut store = SettingsStore::in_memory(AppSettings::default());
    store.update(|s| {
        s.no_wifi_timeout_seconds = 60;
        s.warn_before_lock = true;
        s.warning_seconds = 10;
    });
    store.add_bluetooth_device(BluetoothDeviceRule::new(
        "미밴드",
        BluetoothDeviceKind::Band,
        None,
        Some("04:34:c3:64:6f:d1"),
        None,
    ));

    let mut ctrl = Controller::new(None);
    let mut locked = false;
    for sec in 0..130 {
        let now = 1000.0 + sec as f64;
        let bt = if sec < 70 {
            vec![obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-58), false, now)]
        } else {
            vec![]
        };
        let r = ctrl.tick(
            store.settings(),
            SystemInputs {
                now,
                awake: now,
                wall: now,
                idle_seconds: Some(sec as f64),
                screen_locked: false,
                assertion_holders: vec![],
                wifi: WiFiSnapshot::status(WiFiStatus::Disconnected),
                bluetooth: bt,
            },
        );
        if sec == 65 {
            assert_eq!(r.output.state, EngineState::HeldByBluetooth { devices: vec!["미밴드".into()] });
        }
        if r.lock_reason.is_some() {
            locked = true;
            assert!(sec >= 105, "신호 만료(25초) + 이탈 유예(15초) + 경고(10초) 후 잠겨야 함 (실제 {sec}초)");
            break;
        }
    }
    assert!(locked, "기기가 멀어지면 유예·경고 후 잠겨야 함");
}

#[test]
fn custom_rssi_threshold_and_low_power_band_stability() {
    // 1) bluetoothCustomRssi 직렬화·역직렬화·클램핑
    let parsed_low: AppSettings = serde_json::from_str(r#"{"bluetoothCustomRssi":-120}"#).unwrap();
    assert_eq!(parsed_low.bluetooth_custom_rssi, Some(-95));
    assert_eq!(parsed_low.effective_bluetooth_rssi(), (-95, -100));

    let parsed_high: AppSettings = serde_json::from_str(r#"{"bluetoothCustomRssi":-10}"#).unwrap();
    assert_eq!(parsed_high.bluetooth_custom_rssi, Some(-45));
    assert_eq!(parsed_high.effective_bluetooth_rssi(), (-45, -53));

    let parsed_custom: AppSettings = serde_json::from_str(r#"{"bluetoothRange":"near","bluetoothCustomRssi":-77}"#).unwrap();
    assert_eq!(parsed_custom.effective_bluetooth_rssi(), (-77, -85));
    assert!(serde_json::to_string(&parsed_custom).unwrap().contains("\"bluetoothCustomRssi\":-77"));

    // 2) 커스텀 임계값(-77 진입 / -85 이탈) 동작
    let mut custom_tracker = BluetoothProximityTracker::new();
    let band = BluetoothDeviceRule::new("미밴드", BluetoothDeviceKind::Band, None, Some("04:34:c3:64:6f:d1"), None);
    let rules = [band];
    let r_miss = custom_tracker.update_with_thresholds(10.0, -77, -85, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-78), false, 10.0)]);
    assert!(r_miss.holding_devices.is_empty());
    let _ = custom_tracker.update_with_thresholds(11.0, -77, -85, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-76), false, 11.0)]);
    let r_hit = custom_tracker.update_with_thresholds(12.0, -77, -85, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-76), false, 12.0)]);
    assert_eq!(r_hit.holding_devices, vec!["미밴드"]);

    // 3) 저전력 밴드(-62 dBm) 실사용: Near(-65/-74)에서 18초 스캔 공백이 생겨도 보류 유지 후 -78 dBm 이탈 시 유예 후 해제
    let mut band_tracker = BluetoothProximityTracker::new();
    let _ = band_tracker.update(10.0, ProximityRange::Near, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-62), false, 10.0)]);
    let rep_enter = band_tracker.update(11.0, ProximityRange::Near, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-62), false, 11.0)]);
    assert_eq!(rep_enter.holding_devices, vec!["미밴드"]);

    for t in 12..=29 {
        let rep_gap = band_tracker.update(t as f64, ProximityRange::Near, &rules, &[]);
        assert_eq!(rep_gap.holding_devices, vec!["미밴드"], "18초 패킷 공백({t}초)에도 유지");
    }

    let mut unlocked = false;
    for sec in 30..=60 {
        let t = sec as f64;
        let rep = band_tracker.update(t, ProximityRange::Near, &rules, &[obs("band", None, Some("04:34:c3:64:6f:d1"), None, Some(-78), false, t)]);
        if rep.holding_devices.is_empty() {
            unlocked = true;
            break;
        }
    }
    assert!(unlocked, "미밴드도 -78 dBm으로 멀어지면 유예 후 보류 해제");
}
