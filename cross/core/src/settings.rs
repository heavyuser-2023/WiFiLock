use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// 등록한 네트워크의 기본 잠금 시간 (30분)
pub const DEFAULT_TRUSTED_TIMEOUT: i64 = 1800;
/// 경고 표시 시간 선택지 (초)
pub const WARNING_PRESETS: [i64; 5] = [5, 10, 15, 30, 60];

/// "d8:7:b6:47:6b:8c" 같은 표기를 "d8:07:b6:47:6b:8c" 로 맞춘다. 윈도우식 "D8-07-B6-…" 도 받는다.
/// 형식이 틀렸거나 의미 없는 주소(모두 0, 브로드캐스트)면 None.
pub fn mac_normalize(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase().replace('-', ":");
    let parts: Vec<&str> = lower.split(':').collect();
    if parts.len() != 6 {
        return None;
    }
    let mut out = Vec::with_capacity(6);
    for p in parts {
        if p.is_empty() || p.len() > 2 {
            return None;
        }
        let v = u8::from_str_radix(p, 16).ok()?;
        out.push(format!("{v:02x}"));
    }
    let joined = out.join(":");
    if joined == "00:00:00:00:00:00" || joined == "ff:ff:ff:ff:ff:ff" {
        return None;
    }
    Some(joined)
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

/// 특정 와이파이 네트워크에 적용할 잠금 시간 규칙.
/// 와이파이 이름으로 찾고, 이름을 읽을 수 없을 때는 공유기(기본 게이트웨이) 하드웨어 주소로 찾는다.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkRule {
    pub id: Uuid,
    /// 목록에 보여 줄 이름 (사용자가 바꿀 수 있음)
    pub name: String,
    /// 와이파이 이름. 이름을 몰랐을 때 추가한 규칙은 None.
    pub ssid: Option<String>,
    /// 공유기 하드웨어 주소 (정규화된 소문자)
    #[serde(rename = "routerMAC")]
    pub router_mac: Option<String>,
    /// 마지막 입력 후 잠글 때까지의 시간(초). 0 이면 잠그지 않는다.
    pub timeout_seconds: i64,
}

impl NetworkRule {
    pub fn new(name: impl Into<String>, ssid: Option<&str>, router_mac: Option<&str>, timeout_seconds: i64) -> Self {
        Self::with_id(Uuid::new_v4(), name, ssid, router_mac, timeout_seconds)
    }

    pub fn with_id(id: Uuid, name: impl Into<String>, ssid: Option<&str>, router_mac: Option<&str>, timeout_seconds: i64) -> Self {
        NetworkRule {
            id,
            name: name.into(),
            ssid: non_empty(ssid.map(str::to_owned)),
            router_mac: router_mac.and_then(mac_normalize),
            timeout_seconds: timeout_seconds.max(0),
        }
    }

    /// 화면에 보여 줄 식별 정보: "와이파이 이름: X · 공유기: …"
    pub fn identifier_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(s) = &self.ssid {
            parts.push(format!("와이파이 이름: {s}"));
        }
        if let Some(m) = &self.router_mac {
            parts.push(format!("공유기: {m}"));
        }
        parts.join(" · ")
    }
}

/// 저장된 값 일부가 없거나 깨져 있어도 읽을 수 있게 느슨하게 읽는다.
impl<'de> Deserialize<'de> for NetworkRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        if !v.is_object() {
            return Err(serde::de::Error::custom("규칙은 객체여야 합니다"));
        }
        let id = v
            .get("id")
            .and_then(|x| x.as_str())
            .and_then(|s| Uuid::parse_str(s).ok())
            .unwrap_or_else(Uuid::new_v4);
        let ssid = non_empty(v.get("ssid").and_then(|x| x.as_str()).map(str::to_owned));
        let router_mac = v.get("routerMAC").and_then(|x| x.as_str()).and_then(mac_normalize);
        let timeout_seconds = v
            .get("timeoutSeconds")
            .and_then(|x| x.as_i64())
            .unwrap_or(DEFAULT_TRUSTED_TIMEOUT)
            .max(0);
        let mut name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_owned();
        if name.is_empty() {
            name = ssid.clone().or_else(|| router_mac.clone()).unwrap_or_else(|| "네트워크".into());
        }
        Ok(NetworkRule { id, name, ssid, router_mac, timeout_seconds })
    }
}

/// 앱 전체 설정
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// 자동 잠금 사용 여부
    pub enabled: bool,
    /// 네트워크별 규칙 (위에서부터 우선)
    pub rules: Vec<NetworkRule>,
    /// 등록되지 않은 와이파이에 연결되었을 때의 잠금 시간(초, 0 = 잠그지 않음)
    #[serde(rename = "unknownWiFiTimeoutSeconds")]
    pub unknown_wifi_timeout_seconds: i64,
    /// 와이파이가 꺼져 있거나 연결되지 않았을 때의 잠금 시간(초, 0 = 잠그지 않음)
    #[serde(rename = "noWiFiTimeoutSeconds")]
    pub no_wifi_timeout_seconds: i64,
    /// 잠그기 직전에 화면에 경고를 띄울지
    pub warn_before_lock: bool,
    /// 경고를 띄우는 시간(초)
    pub warning_seconds: i64,
    /// 동영상 재생처럼 앱이 화면 유지를 요청한 동안은 잠그지 않기
    pub respect_display_sleep_assertions: bool,
    /// 메뉴 막대 아이콘 옆에 남은 시간 표시 (맥에서만 보임)
    pub show_countdown_in_menu_bar: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            enabled: true,
            rules: Vec::new(),
            unknown_wifi_timeout_seconds: 300,
            no_wifi_timeout_seconds: 180,
            warn_before_lock: true,
            warning_seconds: 10,
            respect_display_sleep_assertions: true,
            show_countdown_in_menu_bar: false,
        }
    }
}

impl AppSettings {
    /// 범위를 벗어난 값을 바로잡는다.
    pub fn clamped(mut self) -> Self {
        self.unknown_wifi_timeout_seconds = self.unknown_wifi_timeout_seconds.max(0);
        self.no_wifi_timeout_seconds = self.no_wifi_timeout_seconds.max(0);
        self.warning_seconds = self.warning_seconds.clamp(1, 300);
        for r in &mut self.rules {
            r.timeout_seconds = r.timeout_seconds.max(0);
            r.ssid = non_empty(r.ssid.take());
            r.router_mac = r.router_mac.as_deref().and_then(mac_normalize);
            if r.name.trim().is_empty() {
                r.name = r.ssid.clone().or_else(|| r.router_mac.clone()).unwrap_or_else(|| "네트워크".into());
            }
        }
        self
    }
}

/// 저장된 값 일부가 없거나 깨져 있어도 기본값으로 채워 읽는다 (버전이 바뀌어도 설정이 날아가지 않도록).
impl<'de> Deserialize<'de> for AppSettings {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let def = AppSettings::default();
        let b = |k: &str, dv: bool| v.get(k).and_then(|x| x.as_bool()).unwrap_or(dv);
        let i = |k: &str, dv: i64| v.get(k).and_then(|x| x.as_i64()).unwrap_or(dv);
        let rules = match v.get("rules").and_then(|x| x.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|r| serde_json::from_value::<NetworkRule>(r.clone()).ok())
                .collect(),
            None => def.rules.clone(),
        };
        Ok(AppSettings {
            enabled: b("enabled", def.enabled),
            rules,
            unknown_wifi_timeout_seconds: i("unknownWiFiTimeoutSeconds", def.unknown_wifi_timeout_seconds),
            no_wifi_timeout_seconds: i("noWiFiTimeoutSeconds", def.no_wifi_timeout_seconds),
            warn_before_lock: b("warnBeforeLock", def.warn_before_lock),
            warning_seconds: i("warningSeconds", def.warning_seconds),
            respect_display_sleep_assertions: b("respectDisplaySleepAssertions", def.respect_display_sleep_assertions),
            show_countdown_in_menu_bar: b("showCountdownInMenuBar", def.show_countdown_in_menu_bar),
        }
        .clamped())
    }
}

/// 설정 화면에서 고를 수 있는 잠금 시간 목록 (초). 0 은 "잠그지 않음".
pub struct TimeoutPresets;

impl TimeoutPresets {
    pub const ALL: [i64; 13] = [30, 60, 120, 180, 300, 600, 900, 1200, 1800, 2700, 3600, 7200, 0];

    /// 현재 값이 목록에 없으면 끼워 넣어 선택 상자가 비지 않게 한다. "잠그지 않음"은 항상 마지막.
    pub fn including(value: i64) -> Vec<i64> {
        if Self::ALL.contains(&value) {
            return Self::ALL.to_vec();
        }
        let mut list: Vec<i64> = Self::ALL.iter().copied().filter(|v| *v != 0).collect();
        if value > 0 {
            list.push(value);
        }
        list.sort_unstable();
        list.push(0);
        list
    }

    /// 경고 시간 선택지 (현재 값 포함)
    pub fn warning_including(value: i64) -> Vec<i64> {
        let mut list = WARNING_PRESETS.to_vec();
        if !list.contains(&value) {
            list.push(value);
            list.sort_unstable();
        }
        list
    }
}
