use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::i18n::{self, Lang};
use crate::settings::mac_normalize;
use crate::Timestamp;

/// 블루투스 근접 감지 거리 3단계 (기본값: 가까움)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProximityRange {
    /// 가까움 (약 1~2m, 책상 앞)
    #[default]
    Near,
    /// 중간 가까움 (약 3~5m, 방 안)
    Medium,
    /// 먼 거리 (약 6~10m, 넓은 범위)
    Far,
}

impl ProximityRange {
    pub const ALL: [ProximityRange; 3] = [ProximityRange::Near, ProximityRange::Medium, ProximityRange::Far];
    pub const CUSTOM_RSSI_MIN: i16 = -95;
    pub const CUSTOM_RSSI_MAX: i16 = -45;
    pub const CUSTOM_HYSTERESIS_DB: i16 = 8;

    pub fn as_str(self) -> &'static str {
        match self {
            ProximityRange::Near => "near",
            ProximityRange::Medium => "medium",
            ProximityRange::Far => "far",
        }
    }

    /// 보류 진입 RSSI 기준값 (dBm, 이 값 이상이면 범위 안)
    pub fn enter_rssi(self) -> i16 {
        match self {
            ProximityRange::Near => -65,
            ProximityRange::Medium => -76,
            ProximityRange::Far => -88,
        }
    }

    /// 보류 이탈 RSSI 기준값 (dBm, 이 값 이하이면 범위 밖)
    pub fn exit_rssi(self) -> i16 {
        match self {
            ProximityRange::Near => -74,
            ProximityRange::Medium => -84,
            ProximityRange::Far => -95,
        }
    }

    pub fn clamp_custom_rssi(rssi: i16) -> i16 {
        rssi.clamp(Self::CUSTOM_RSSI_MIN, Self::CUSTOM_RSSI_MAX)
    }

    pub fn thresholds(self, custom_enter: Option<i16>) -> (i16, i16) {
        match custom_enter {
            Some(raw) => {
                let enter = Self::clamp_custom_rssi(raw);
                (enter, (enter - Self::CUSTOM_HYSTERESIS_DB).max(-100))
            }
            None => (self.enter_rssi(), self.exit_rssi()),
        }
    }

    pub fn label(self) -> &'static str {
        self.label_in(i18n::current())
    }

    pub fn label_in(self, lang: Lang) -> &'static str {
        match (lang, self) {
            (Lang::Ko, ProximityRange::Near) => "가까움 (약 1~2m)",
            (Lang::Ko, ProximityRange::Medium) => "중간 가까움 (약 3~5m)",
            (Lang::Ko, ProximityRange::Far) => "먼 거리 (약 6~10m)",
            (Lang::En, ProximityRange::Near) => "Near (~1–2m)",
            (Lang::En, ProximityRange::Medium) => "Medium (~3–5m)",
            (Lang::En, ProximityRange::Far) => "Far (~6–10m)",
        }
    }

    pub fn description_in(self, lang: Lang) -> &'static str {
        match (lang, self) {
            (Lang::Ko, ProximityRange::Near) => "책상 앞(약 1~2m)을 벗어나면 자동 잠금이 다시 동작합니다.",
            (Lang::Ko, ProximityRange::Medium) => "방 안(약 3~5m)을 벗어나면 자동 잠금이 다시 동작합니다.",
            (Lang::Ko, ProximityRange::Far) => "먼 거리(약 6~10m)를 벗어나 신호가 약해지면 자동 잠금이 다시 동작합니다.",
            (Lang::En, ProximityRange::Near) => "Auto-lock resumes when your device moves beyond your desk (~1–2m).",
            (Lang::En, ProximityRange::Medium) => "Auto-lock resumes when your device leaves the room (~3–5m).",
            (Lang::En, ProximityRange::Far) => "Auto-lock resumes when your device moves beyond wider range (~6–10m).",
        }
    }
}

impl<'de> Deserialize<'de> for ProximityRange {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d).unwrap_or_default();
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "medium" | "room" => ProximityRange::Medium,
            "far" => ProximityRange::Far,
            "near" | "desk" => ProximityRange::Near,
            _ => ProximityRange::Near,
        })
    }
}

/// 화면에 아이콘을 고르기 위한 기기 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BluetoothDeviceKind {
    Phone,
    Band,
    Audio,
    #[default]
    Other,
}

impl<'de> Deserialize<'de> for BluetoothDeviceKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d).unwrap_or_default();
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "phone" => BluetoothDeviceKind::Phone,
            "band" => BluetoothDeviceKind::Band,
            "audio" => BluetoothDeviceKind::Audio,
            _ => BluetoothDeviceKind::Other,
        })
    }
}

/// 이름·블루투스 클래스·MiBeacon·아이콘 힌트로 기기 종류를 추론한다.
pub fn infer_device_kind(
    name: Option<&str>,
    major_class: Option<u32>,
    minor_class: Option<u32>,
    is_mibeacon: bool,
    icon_hint: Option<&str>,
) -> BluetoothDeviceKind {
    let _ = minor_class;
    let lower = name.unwrap_or("").to_ascii_lowercase();
    let icon = icon_hint.unwrap_or("").to_ascii_lowercase();

    if is_mibeacon
        || ["band", "watch", "mi smart", "fitbit", "garmin", "miband"]
            .iter()
            .any(|k| lower.contains(k))
    {
        return BluetoothDeviceKind::Band;
    }

    if major_class == Some(2)
        || icon.contains("phone")
        || ["iphone", "pixel", "galaxy s", "galaxy z", "phone"]
            .iter()
            .any(|k| lower.contains(k))
    {
        return BluetoothDeviceKind::Phone;
    }

    if major_class == Some(4)
        || icon.contains("audio")
        || icon.contains("head")
        || ["airpods", "buds", "headphone", "earbud", "ear (", "bose", "wh-", "wf-"]
            .iter()
            .any(|k| lower.contains(k))
    {
        return BluetoothDeviceKind::Audio;
    }

    BluetoothDeviceKind::Other
}

/// Xiaomi MiBeacon (0xFE95) 서비스 데이터에서 MAC 주소를 꺼낸다.
/// Frame Control(리틀 엔디언 u16)의 bit 4 (0x0010, HAS_MAC)가 1이면 바이트 5..11에 역순 MAC이 들어 있다.
pub fn parse_mibeacon_mac(service_data: &[u8]) -> Option<String> {
    if service_data.len() < 11 {
        return None;
    }
    let frame_ctrl = u16::from_le_bytes([service_data[0], service_data[1]]);
    if frame_ctrl & 0x0010 == 0 {
        return None;
    }
    let m = &service_data[5..11];
    mac_normalize(&format!(
        "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        m[5], m[4], m[3], m[2], m[1], m[0]
    ))
}

fn non_empty_trimmed(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// 사용자가 잠금 보류 대상으로 등록한 블루투스 기기 규칙
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothDeviceRule {
    pub id: Uuid,
    pub name: String,
    pub kind: BluetoothDeviceKind,
    /// macOS CoreBluetooth UUID 또는 Windows/Linux BLE 식별자
    pub ble_id: Option<String>,
    /// 패킷(예: MiBeacon 0xFE95) 또는 BLE에서 읽은 고정 MAC 주소
    pub mac_address: Option<String>,
    /// OS 페어링 목록의 Classic Bluetooth 주소
    pub classic_address: Option<String>,
    pub enabled: bool,
}

impl BluetoothDeviceRule {
    pub fn new(
        name: impl Into<String>,
        kind: BluetoothDeviceKind,
        ble_id: Option<&str>,
        mac_address: Option<&str>,
        classic_address: Option<&str>,
    ) -> Self {
        Self::with_id(Uuid::new_v4(), name, kind, ble_id, mac_address, classic_address, true)
    }

    pub fn with_id(
        id: Uuid,
        name: impl Into<String>,
        kind: BluetoothDeviceKind,
        ble_id: Option<&str>,
        mac_address: Option<&str>,
        classic_address: Option<&str>,
        enabled: bool,
    ) -> Self {
        let mut rule = BluetoothDeviceRule {
            id,
            name: name.into(),
            kind,
            ble_id: non_empty_trimmed(ble_id),
            mac_address: mac_address.and_then(mac_normalize),
            classic_address: classic_address.and_then(mac_normalize),
            enabled,
        };
        rule.sanitize();
        rule
    }

    pub fn sanitize(&mut self) {
        self.ble_id = non_empty_trimmed(self.ble_id.as_deref());
        self.mac_address = self.mac_address.as_deref().and_then(mac_normalize);
        self.classic_address = self.classic_address.as_deref().and_then(mac_normalize);
        let trimmed = self.name.trim().to_owned();
        if trimmed.is_empty() {
            self.name = self
                .mac_address
                .clone()
                .or_else(|| self.classic_address.clone())
                .or_else(|| self.ble_id.clone())
                .unwrap_or_else(|| i18n::pick("블루투스 기기", "Bluetooth Device").into());
        } else {
            self.name = trimmed;
        }
    }

    pub fn has_any_identifier(&self) -> bool {
        self.ble_id.is_some() || self.mac_address.is_some() || self.classic_address.is_some()
    }

    /// 두 규칙이 같은 하드웨어 식별자를 공유하는지
    pub fn same_device_as(&self, other: &BluetoothDeviceRule) -> bool {
        let addr_match = [self.mac_address.as_deref(), self.classic_address.as_deref()]
            .into_iter()
            .flatten()
            .any(|a| [other.mac_address.as_deref(), other.classic_address.as_deref()].into_iter().flatten().any(|b| a == b));
        if addr_match {
            return true;
        }
        match (&self.ble_id, &other.ble_id) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }

    /// 관측된 블루투스 기기와 식별자가 일치하는지 (이름만으로는 매칭하지 않는다)
    pub fn matches(&self, obs: &BluetoothObservation) -> bool {
        let addr_match = [self.mac_address.as_deref(), self.classic_address.as_deref()]
            .into_iter()
            .flatten()
            .any(|a| [obs.mac_address.as_deref(), obs.classic_address.as_deref()].into_iter().flatten().any(|b| a == b));
        if addr_match {
            return true;
        }
        match (&self.ble_id, &obs.ble_id) {
            (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }

    /// 설정 목록 아래에 보여 줄 식별 문구
    pub fn identifier_text_in(&self, lang: Lang) -> String {
        let mut parts = Vec::new();
        if let Some(m) = &self.mac_address {
            parts.push(format!("MAC {m}"));
        }
        if let Some(c) = &self.classic_address {
            if self.mac_address.as_deref() != Some(c.as_str()) {
                parts.push(match lang {
                    Lang::Ko => format!("페어링 {c}"),
                    Lang::En => format!("Paired {c}"),
                });
            }
        }
        if parts.is_empty() {
            if let Some(b) = &self.ble_id {
                let short: String = b.chars().take(8).collect();
                parts.push(format!("BLE {short}"));
            }
        }
        parts.join(" · ")
    }
}

impl<'de> Deserialize<'de> for BluetoothDeviceRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if !v.is_object() {
            return Err(serde::de::Error::custom("블루투스 기기 규칙은 객체여야 합니다"));
        }
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);
        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_owned();
        let kind = v
            .get("kind")
            .and_then(|x| serde_json::from_value::<BluetoothDeviceKind>(x.clone()).ok())
            .unwrap_or_default();
        let ble_id = v.get("bleId").and_then(|x| x.as_str()).map(str::to_owned);
        let mac_address = v.get("macAddress").and_then(|x| x.as_str()).and_then(mac_normalize);
        let classic_address = v.get("classicAddress").and_then(|x| x.as_str()).and_then(mac_normalize);
        let enabled = v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true);
        let mut rule = BluetoothDeviceRule {
            id,
            name,
            kind,
            ble_id,
            mac_address,
            classic_address,
            enabled,
        };
        rule.sanitize();
        if !rule.has_any_identifier() {
            return Err(serde::de::Error::custom("식별자가 없는 블루투스 기기 규칙"));
        }
        Ok(rule)
    }
}

/// 운영체제 블루투스 계층에서 수집한 기기 관측값
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothObservation {
    pub key: String,
    pub name: Option<String>,
    pub kind: BluetoothDeviceKind,
    pub ble_id: Option<String>,
    pub mac_address: Option<String>,
    pub classic_address: Option<String>,
    pub rssi: Option<i16>,
    pub connected: bool,
    pub paired: bool,
    pub seen_at: Timestamp,
}

impl BluetoothObservation {
    /// 유효한 RSSI 범위(-120 ~ -1 dBm)만 남기고 0, 127 등 무효값은 None 처리한다.
    pub fn normalize_rssi(raw: i16) -> Option<i16> {
        (-120..=-1).contains(&raw).then_some(raw)
    }
}

/// 설정 UI에 전달할 등록 기기별 실시간 근접 상태
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothDeviceLive {
    pub rule_id: Uuid,
    pub in_range: bool,
    pub holding: bool,
    pub rssi: Option<i16>,
    pub connected: bool,
    pub last_seen_ago_seconds: Option<f64>,
    pub leaving_remaining_seconds: Option<f64>,
}

/// 근접 추적기 평가 결과
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProximityReport {
    pub holding_devices: Vec<String>,
    pub live: Vec<BluetoothDeviceLive>,
    pub any_leaving: bool,
}

#[derive(Debug, Clone, Default)]
struct TrackerDeviceState {
    samples: Vec<(Timestamp, i16)>,
    last_obs_seen_at: Option<Timestamp>,
    last_seen: Option<Timestamp>,
    last_valid_rssi_at: Option<Timestamp>,
    holding: bool,
    enter_streak: u32,
    enter_since: Option<Timestamp>,
    leaving_since: Option<Timestamp>,
}

impl TrackerDeviceState {
    fn median_rssi(&self) -> Option<i16> {
        if self.samples.is_empty() {
            return None;
        }
        let mut vals: Vec<i16> = self.samples.iter().map(|(_, r)| *r).collect();
        vals.sort_unstable();
        let mid = vals.len() / 2;
        if vals.len() % 2 == 1 {
            Some(vals[mid])
        } else {
            let sum = i32::from(vals[mid - 1]) + i32::from(vals[mid]);
            Some((sum / 2) as i16)
        }
    }
}

/// RSSI 중앙값 필터, 히스테리시스, 15초 이탈 유예로 깜빡임 없이 근접 여부를 판정한다.
#[derive(Debug, Default)]
pub struct BluetoothProximityTracker {
    states: HashMap<Uuid, TrackerDeviceState>,
}

impl BluetoothProximityTracker {
    pub const SAMPLE_WINDOW_SECS: f64 = 20.0;
    pub const MAX_SAMPLES: usize = 5;
    pub const STALE_SECS: f64 = 25.0;
    pub const LEAVE_GRACE_SECS: f64 = 15.0;
    pub const CONNECTED_RSSI_FALLBACK_SECS: f64 = 20.0;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }

    pub fn update(
        &mut self,
        now: Timestamp,
        range: ProximityRange,
        rules: &[BluetoothDeviceRule],
        observations: &[BluetoothObservation],
    ) -> ProximityReport {
        self.update_with_thresholds(now, range.enter_rssi(), range.exit_rssi(), rules, observations)
    }

    pub fn update_with_thresholds(
        &mut self,
        now: Timestamp,
        enter_rssi: i16,
        exit_rssi: i16,
        rules: &[BluetoothDeviceRule],
        observations: &[BluetoothObservation],
    ) -> ProximityReport {
        self.states.retain(|id, _| rules.iter().any(|r| r.id == *id));

        let mut report = ProximityReport::default();

        for rule in rules {
            if !rule.enabled {
                self.states.remove(&rule.id);
                report.live.push(BluetoothDeviceLive {
                    rule_id: rule.id,
                    in_range: false,
                    holding: false,
                    rssi: None,
                    connected: false,
                    last_seen_ago_seconds: None,
                    leaving_remaining_seconds: None,
                });
                continue;
            }

            let st = self.states.entry(rule.id).or_default();

            // 매칭되는 관측값 병합 선택 (유효 RSSI 우선, 더 최근 관측 우선, connected OR)
            let matched: Vec<&BluetoothObservation> = observations.iter().filter(|o| rule.matches(o)).collect();
            let connected = matched.iter().any(|o| o.connected);
            let best_seen = matched.iter().map(|o| o.seen_at).fold(None, |acc: Option<f64>, t| {
                Some(acc.map_or(t, |a| a.max(t)))
            });
            let best_rssi_obs = matched
                .iter()
                .filter(|o| o.rssi.and_then(BluetoothObservation::normalize_rssi).is_some())
                .max_by(|a, b| a.seen_at.total_cmp(&b.seen_at));

            if let Some(seen) = best_seen {
                if now - seen <= Self::STALE_SECS {
                    st.last_seen = Some(st.last_seen.map_or(seen, |prev| prev.max(seen)));
                }
            }

            if let Some(robs) = best_rssi_obs {
                if let Some(rssi) = robs.rssi.and_then(BluetoothObservation::normalize_rssi) {
                    let is_new_sample = st.last_obs_seen_at.is_none_or(|prev| robs.seen_at > prev + 0.05);
                    if is_new_sample && now - robs.seen_at <= Self::STALE_SECS {
                        st.samples.push((robs.seen_at, rssi));
                        st.last_obs_seen_at = Some(robs.seen_at);
                        st.last_valid_rssi_at = Some(robs.seen_at);
                    }
                }
            }

            st.samples.retain(|(t, _)| now - *t <= Self::SAMPLE_WINDOW_SECS && *t <= now + 5.0);
            if st.samples.len() > Self::MAX_SAMPLES {
                let drop_n = st.samples.len() - Self::MAX_SAMPLES;
                st.samples.drain(0..drop_n);
            }

            let median_rssi = st.median_rssi();
            let is_fresh = st.last_seen.is_some_and(|t| now - t <= Self::STALE_SECS);
            let rssi_absent_long = st
                .last_valid_rssi_at
                .is_none_or(|t| now - t > Self::CONNECTED_RSSI_FALLBACK_SECS);

            let in_range = if !is_fresh {
                false
            } else if let Some(m) = median_rssi {
                // OS에 이미 연결된 기기(예: 에어팟)는 책상 앞에서도 진입 문턱을 강제하지 않고
                // 이탈 문턱을 초과하는 동안 범위 안으로 인정한다.
                if st.holding || connected {
                    m > exit_rssi
                } else {
                    m >= enter_rssi
                }
            } else {
                connected && rssi_absent_long
            };

            if in_range {
                st.leaving_since = None;
                if !st.holding {
                    st.enter_streak = st.enter_streak.saturating_add(1);
                    let first_enter = *st.enter_since.get_or_insert(now);
                    let strong = median_rssi.is_some_and(|m| m >= enter_rssi + 4);
                    if connected || strong || st.enter_streak >= 2 || now - first_enter >= 1.5 {
                        st.holding = true;
                    }
                }
            } else {
                st.enter_streak = 0;
                st.enter_since = None;
                if st.holding {
                    let since = *st.leaving_since.get_or_insert(now);
                    if now - since >= Self::LEAVE_GRACE_SECS {
                        st.holding = false;
                        st.leaving_since = None;
                    }
                }
            }

            let leaving_remaining = if st.holding {
                st.leaving_since.map(|since| (Self::LEAVE_GRACE_SECS - (now - since)).max(0.0))
            } else {
                None
            };
            if leaving_remaining.is_some() {
                report.any_leaving = true;
            }
            if st.holding {
                report.holding_devices.push(rule.name.clone());
            }

            report.live.push(BluetoothDeviceLive {
                rule_id: rule.id,
                in_range,
                holding: st.holding,
                rssi: median_rssi,
                connected,
                last_seen_ago_seconds: st.last_seen.map(|t| (now - t).max(0.0)),
                leaving_remaining_seconds: leaving_remaining,
            });
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_mibeacon_mac() {
        // Xiaomi Smart Band 10 6FD1 실측 패킷: FE95 serviceData = 1359ff5908d16f64c33404
        let raw = [0x13u8, 0x59, 0xff, 0x59, 0x08, 0xd1, 0x6f, 0x64, 0xc3, 0x34, 0x04];
        assert_eq!(parse_mibeacon_mac(&raw).as_deref(), Some("04:34:c3:64:6f:d1"));

        // HAS_MAC(0x0010) 비트가 꺼져 있으면 None
        let no_mac = [0x03u8, 0x59, 0xff, 0x59, 0x08, 0xd1, 0x6f, 0x64, 0xc3, 0x34, 0x04];
        assert_eq!(parse_mibeacon_mac(&no_mac), None);
        assert_eq!(parse_mibeacon_mac(&raw[..10]), None);
    }

    #[test]
    fn infers_device_kinds() {
        assert_eq!(infer_device_kind(Some("Xiaomi Smart Band 10 6FD1"), None, None, true, None), BluetoothDeviceKind::Band);
        assert_eq!(infer_device_kind(Some("heavyuser"), Some(2), Some(3), false, None), BluetoothDeviceKind::Phone);
        assert_eq!(infer_device_kind(Some("Heavyuser's AirPods Pro"), Some(4), Some(6), false, None), BluetoothDeviceKind::Audio);
        assert_eq!(infer_device_kind(Some("Galaxy Buds3 Pro LE"), None, None, false, None), BluetoothDeviceKind::Audio);
        assert_eq!(infer_device_kind(Some("Keychron Q60"), None, None, false, None), BluetoothDeviceKind::Other);
    }

    #[test]
    fn proximity_range_deserializes_legacy_aliases() {
        let near: ProximityRange = serde_json::from_str("\"desk\"").unwrap();
        let med: ProximityRange = serde_json::from_str("\"room\"").unwrap();
        let far: ProximityRange = serde_json::from_str("\"far\"").unwrap();
        let unk: ProximityRange = serde_json::from_str("\"unknown\"").unwrap();
        assert_eq!(near, ProximityRange::Near);
        assert_eq!(med, ProximityRange::Medium);
        assert_eq!(far, ProximityRange::Far);
        assert_eq!(unk, ProximityRange::Near);
    }
}
