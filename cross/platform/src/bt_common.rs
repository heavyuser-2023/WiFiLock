//! 운영체제들이 공유하는 블루투스 관측 병합 및 주소 변환 유틸

use wifilock_core::{mac_normalize, BluetoothDeviceKind, BluetoothObservation};

/// 48비트 블루투스 주소(`u64`, 빅엔디언 바이트 순서의 하위 6바이트)를 정규화 MAC 문자열로 바꾼다.
pub fn mac_from_u64_be(addr: u64) -> Option<String> {
    let b = addr.to_be_bytes();
    mac_normalize(&format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[2], b[3], b[4], b[5], b[6], b[7]
    ))
}

/// 리틀 엔디언 바이트 배열(예: Win32 `BLUETOOTH_ADDRESS.rgBytes`)을 정규화 MAC 문자열로 바꾼다.
pub fn mac_from_bytes_rev(b: &[u8]) -> Option<String> {
    if b.len() < 6 {
        return None;
    }
    mac_normalize(&format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        b[5], b[4], b[3], b[2], b[1], b[0]
    ))
}

fn clean_name(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

fn names_exact_match(paired_name: &str, ble_name: &str) -> bool {
    let p = paired_name.trim().to_ascii_lowercase();
    let b = ble_name.trim().to_ascii_lowercase();
    !p.is_empty() && p == b
}

fn names_fuzzy_match(
    paired_name: &str,
    ble_name: &str,
    paired_kind: BluetoothDeviceKind,
    ble_kind: BluetoothDeviceKind,
) -> bool {
    let p = paired_name.trim().to_ascii_lowercase();
    let b = ble_name.trim().to_ascii_lowercase();
    if p.is_empty() || b.is_empty() {
        return false;
    }
    // AirPods처럼 페어링 이름("AirPods Pro")이 BLE 광고 이름("Heavyuser’s AirPods Pro")에 포함되는 경우
    // (소유자 이름이 겹치는 다른 기기 오인을 막기 위해 두 기기 종류가 같을 때만 부분 일치를 허용한다)
    if matches!(
        paired_kind,
        BluetoothDeviceKind::Audio | BluetoothDeviceKind::Phone | BluetoothDeviceKind::Band
    ) && paired_kind == ble_kind
        && p.len() >= 5
        && b.len() >= 5
    {
        return b.contains(&p) || p.contains(&b);
    }
    false
}

fn shares_hw_identifier(a: &BluetoothObservation, b: &BluetoothObservation) -> bool {
    let addr_match = [a.mac_address.as_deref(), a.classic_address.as_deref()]
        .into_iter()
        .flatten()
        .any(|x| [b.mac_address.as_deref(), b.classic_address.as_deref()].into_iter().flatten().any(|y| x == y));
    if addr_match {
        return true;
    }
    match (&a.ble_id, &b.ble_id) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        _ => false,
    }
}

fn merge_pair(dst: &mut BluetoothObservation, src: BluetoothObservation) {
    let dst_name = clean_name(dst.name.as_deref());
    let src_name = clean_name(src.name.as_deref());
    dst.name = match (dst_name, src_name) {
        (None, s) => s,
        (d, None) => d,
        (Some(d), Some(s)) => Some(if s.len() > d.len() && src.paired { s } else { d }),
    };
    if dst.kind == BluetoothDeviceKind::Other && src.kind != BluetoothDeviceKind::Other {
        dst.kind = src.kind;
    }
    if dst.ble_id.is_none() {
        dst.ble_id = src.ble_id;
    }
    if dst.mac_address.is_none() {
        dst.mac_address = src.mac_address;
    }
    if dst.classic_address.is_none() {
        dst.classic_address = src.classic_address;
    }
    dst.connected = dst.connected || src.connected;
    dst.paired = dst.paired || src.paired;

    let src_has_rssi = src.rssi.and_then(BluetoothObservation::normalize_rssi).is_some();
    let dst_has_rssi = dst.rssi.and_then(BluetoothObservation::normalize_rssi).is_some();
    let take_src_rssi = src_has_rssi && (!dst_has_rssi || src.seen_at >= dst.seen_at);
    if take_src_rssi {
        dst.rssi = src.rssi;
    }
    if src.seen_at > dst.seen_at {
        dst.seen_at = src.seen_at;
    }
}

/// BLE 광고 관측 목록과 OS 페어링 목록을 하나의 기기 목록으로 병합한다.
pub fn merge_observations(ble: Vec<BluetoothObservation>, paired: Vec<BluetoothObservation>) -> Vec<BluetoothObservation> {
    let mut out: Vec<BluetoothObservation> = Vec::with_capacity(paired.len() + ble.len());

    for mut p in paired {
        p.name = clean_name(p.name.as_deref());
        if let Some(existing) = out.iter_mut().find(|e| shares_hw_identifier(e, &p)) {
            merge_pair(existing, p);
        } else {
            out.push(p);
        }
    }

    for mut b in ble {
        b.name = clean_name(b.name.as_deref());
        if let Some(existing) = out.iter_mut().find(|e| shares_hw_identifier(e, &b)) {
            merge_pair(existing, b);
            continue;
        }
        if let Some(b_name) = b.name.as_deref() {
            if let Some(existing) = out
                .iter_mut()
                .find(|e| e.paired && e.name.as_deref().is_some_and(|pn| names_exact_match(pn, b_name)))
            {
                merge_pair(existing, b);
                continue;
            }
            if let Some(existing) = out.iter_mut().find(|e| {
                e.paired
                    && e.name
                        .as_deref()
                        .is_some_and(|pn| names_fuzzy_match(pn, b_name, e.kind, b.kind))
            }) {
                merge_pair(existing, b);
                continue;
            }
        }
        out.push(b);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_paired(addr: &str, name: &str, kind: BluetoothDeviceKind, rssi: Option<i16>, connected: bool, seen_at: f64) -> BluetoothObservation {
        BluetoothObservation {
            key: format!("paired:{addr}"),
            name: Some(name.into()),
            kind,
            ble_id: None,
            mac_address: None,
            classic_address: Some(addr.into()),
            rssi,
            connected,
            paired: true,
            seen_at,
        }
    }

    fn mk_ble(ble: &str, name: &str, kind: BluetoothDeviceKind, mac: Option<&str>, rssi: i16, seen_at: f64) -> BluetoothObservation {
        BluetoothObservation {
            key: format!("ble:{ble}"),
            name: Some(name.into()),
            kind,
            ble_id: Some(ble.into()),
            mac_address: mac.map(str::to_owned),
            classic_address: None,
            rssi: Some(rssi),
            connected: false,
            paired: false,
            seen_at,
        }
    }

    #[test]
    fn converts_windows_mac_formats() {
        assert_eq!(mac_from_u64_be(0x0000_0434_c364_6fd1).as_deref(), Some("04:34:c3:64:6f:d1"));
        assert_eq!(mac_from_bytes_rev(&[0xd1, 0x6f, 0x64, 0xc3, 0x34, 0x04]).as_deref(), Some("04:34:c3:64:6f:d1"));
        assert_eq!(mac_from_bytes_rev(&[0; 6]), None);
    }

    #[test]
    fn merges_real_iphone_airpods_and_miband() {
        let paired = vec![
            mk_paired("64:6d:2f:a7:28:fc", "heavyuser", BluetoothDeviceKind::Phone, Some(-61), false, 100.0),
            mk_paired("cc:22:fe:38:f9:c4", "AirPods Pro", BluetoothDeviceKind::Audio, None, true, 100.0),
        ];
        let ble = vec![
            mk_ble("3B88C703", "heavyuser", BluetoothDeviceKind::Phone, None, -54, 102.0),
            mk_ble("F6C17078", "Heavyuser’s AirPods Pro", BluetoothDeviceKind::Audio, None, -52, 101.0),
            mk_ble("89CC216D", "Heavyuser’s AirPods Pro", BluetoothDeviceKind::Audio, None, -61, 100.5),
            mk_ble("7B7B8BF0", "Xiaomi Smart Band 10 6FD1", BluetoothDeviceKind::Band, Some("04:34:c3:64:6f:d1"), -57, 102.0),
        ];

        let merged = merge_observations(ble, paired);
        assert_eq!(merged.len(), 3, "아이폰·에어팟·미밴드 3개로 합쳐져야 함");

        let iphone = merged.iter().find(|d| d.classic_address.as_deref() == Some("64:6d:2f:a7:28:fc")).unwrap();
        assert_eq!(iphone.ble_id.as_deref(), Some("3B88C703"));
        assert_eq!(iphone.rssi, Some(-54));

        let airpods = merged.iter().find(|d| d.classic_address.as_deref() == Some("cc:22:fe:38:f9:c4")).unwrap();
        assert!(airpods.connected);
        assert_eq!(airpods.rssi, Some(-52));

        let band = merged.iter().find(|d| d.mac_address.as_deref() == Some("04:34:c3:64:6f:d1")).unwrap();
        assert!(!band.paired);
        assert_eq!(band.rssi, Some(-57));
    }

    #[test]
    fn keeps_newer_profiler_rssi_over_older_ble() {
        let paired = vec![mk_paired(
            "64:6d:2f:a7:28:fc",
            "heavyuser",
            BluetoothDeviceKind::Phone,
            Some(-56),
            false,
            120.0,
        )];
        let ble = vec![mk_ble("3B88C703", "heavyuser", BluetoothDeviceKind::Phone, None, -63, 110.0)];

        let merged = merge_observations(ble, paired);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].rssi, Some(-56));
        assert_eq!(merged[0].seen_at, 120.0);
        assert_eq!(merged[0].ble_id.as_deref(), Some("3B88C703"));
    }

    #[test]
    fn does_not_steal_iphone_ble_into_airpods_with_shared_owner_name() {
        let paired = vec![
            mk_paired(
                "cc:22:fe:38:f9:c4",
                "Heavyuser’s AirPods Pro",
                BluetoothDeviceKind::Audio,
                None,
                false,
                0.0,
            ),
            mk_paired(
                "64:6d:2f:a7:28:fc",
                "heavyuser",
                BluetoothDeviceKind::Phone,
                None,
                false,
                0.0,
            ),
        ];
        let ble = vec![mk_ble(
            "3B88C703",
            "heavyuser",
            BluetoothDeviceKind::Other,
            None,
            -55,
            102.0,
        )];

        let merged = merge_observations(ble.clone(), paired);
        assert_eq!(merged.len(), 2);

        let airpods = merged.iter().find(|d| d.classic_address.as_deref() == Some("cc:22:fe:38:f9:c4")).unwrap();
        assert_eq!(airpods.ble_id, None, "에어팟이 아이폰 BLE UUID를 납치하면 안 됨");
        assert_eq!(airpods.rssi, None);

        let iphone = merged.iter().find(|d| d.classic_address.as_deref() == Some("64:6d:2f:a7:28:fc")).unwrap();
        assert_eq!(iphone.ble_id.as_deref(), Some("3B88C703"));
        assert_eq!(iphone.rssi, Some(-55));

        // 페어링 목록에 에어팟만 있더라도 kind=Other인 BLE "heavyuser"가 에어팟과 합쳐지면 안 됨
        let only_airpods = vec![mk_paired(
            "cc:22:fe:38:f9:c4",
            "Heavyuser’s AirPods Pro",
            BluetoothDeviceKind::Audio,
            None,
            false,
            0.0,
        )];
        let separate = merge_observations(ble, only_airpods);
        assert_eq!(separate.len(), 2);
        assert_eq!(separate[0].ble_id, None);
        assert_eq!(separate[1].ble_id.as_deref(), Some("3B88C703"));
    }
}
