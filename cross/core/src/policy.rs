use serde::{Deserialize, Serialize};

use crate::settings::{mac_normalize, AppSettings, NetworkRule, DEFAULT_TRUSTED_TIMEOUT};

/// 와이파이 연결 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WiFiStatus {
    /// 와이파이 장치가 없음
    Unavailable,
    /// 와이파이가 꺼져 있음
    PoweredOff,
    /// 켜져 있지만 어떤 네트워크에도 연결되지 않음
    Disconnected,
    /// 네트워크에 연결됨
    Connected,
}

/// 특정 시점의 와이파이 상태
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WiFiSnapshot {
    pub status: WiFiStatus,
    /// 와이파이 이름. 권한이 없으면 연결되어 있어도 None.
    pub ssid: Option<String>,
    /// 공유기(기본 게이트웨이) 하드웨어 주소 (정규화된 값)
    #[serde(rename = "routerMAC")]
    pub router_mac: Option<String>,
    pub interface_name: Option<String>,
}

impl Default for WiFiSnapshot {
    fn default() -> Self {
        Self::unknown()
    }
}

impl WiFiSnapshot {
    pub fn new(status: WiFiStatus, ssid: Option<&str>, router_mac: Option<&str>, interface_name: Option<&str>) -> Self {
        WiFiSnapshot {
            status,
            ssid: ssid.filter(|s| !s.is_empty()).map(str::to_owned),
            router_mac: router_mac.and_then(mac_normalize),
            interface_name: interface_name.map(str::to_owned),
        }
    }

    pub fn status(status: WiFiStatus) -> Self {
        Self::new(status, None, None, None)
    }

    pub fn connected(ssid: Option<&str>, router_mac: Option<&str>) -> Self {
        Self::new(WiFiStatus::Connected, ssid, router_mac, None)
    }

    pub fn unknown() -> Self {
        Self::status(WiFiStatus::Disconnected)
    }

    pub fn is_connected(&self) -> bool {
        self.status == WiFiStatus::Connected
    }

    /// 메뉴에 보여 줄 연결 상태 문구
    pub fn korean_description(&self) -> String {
        match self.status {
            WiFiStatus::Unavailable => "와이파이 장치 없음".into(),
            WiFiStatus::PoweredOff => "와이파이 꺼짐".into(),
            WiFiStatus::Disconnected => "와이파이 연결 안 됨".into(),
            WiFiStatus::Connected => match &self.ssid {
                Some(s) => s.clone(),
                None => "이름 확인 불가 (권한 필요)".into(),
            },
        }
    }
}

/// 현재 적용되는 잠금 시간의 근거
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PolicySource {
    Rule { rule: NetworkRule },
    #[serde(rename = "unknownWiFi")]
    UnknownWiFi,
    #[serde(rename = "noWiFi")]
    NoWiFi,
}

impl PolicySource {
    pub fn korean_description(&self) -> String {
        match self {
            PolicySource::Rule { rule } => format!("등록된 네트워크 '{}'", rule.name),
            PolicySource::UnknownWiFi => "등록되지 않은 와이파이".into(),
            PolicySource::NoWiFi => "와이파이 연결 없음".into(),
        }
    }

    pub fn is_rule(&self) -> bool {
        matches!(self, PolicySource::Rule { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockPolicy {
    /// 잠금까지 시간(초). 0 이하면 잠그지 않는다.
    pub timeout_seconds: i64,
    pub source: PolicySource,
}

impl LockPolicy {
    pub fn new(timeout_seconds: i64, source: PolicySource) -> Self {
        LockPolicy { timeout_seconds, source }
    }

    pub fn never_locks(&self) -> bool {
        self.timeout_seconds <= 0
    }
}

pub struct PolicyResolver;

impl PolicyResolver {
    /// 현재 와이파이 상태에 맞는 규칙을 찾는다.
    ///
    /// - 와이파이 이름을 알면 이름이 같은 규칙을 먼저 찾고, 없으면 "이름 없이 공유기 주소로만 등록된" 규칙을 찾는다.
    ///   (같은 공유기가 손님용·내부용 네트워크를 함께 내보내는 경우 서로 섞이지 않게 하려는 것)
    /// - 이름을 모르면 공유기 주소가 같은 규칙을 찾는다.
    /// - 이름이 같아도 규칙과 현재 상태 모두 공유기 주소를 알고 있는데 서로 다르면 다른 네트워크로 본다.
    pub fn matching_rule<'a>(snapshot: &WiFiSnapshot, rules: &'a [NetworkRule]) -> Option<&'a NetworkRule> {
        if !snapshot.is_connected() {
            return None;
        }
        if let Some(ssid) = &snapshot.ssid {
            let same_router = |r: &NetworkRule| match (&r.router_mac, &snapshot.router_mac) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            };
            if let Some(r) = rules.iter().find(|r| r.ssid.as_deref() == Some(ssid.as_str()) && same_router(r)) {
                return Some(r);
            }
            if let Some(mac) = &snapshot.router_mac {
                return rules.iter().find(|r| r.ssid.is_none() && r.router_mac.as_deref() == Some(mac.as_str()));
            }
            return None;
        }
        if let Some(mac) = &snapshot.router_mac {
            return rules.iter().find(|r| r.router_mac.as_deref() == Some(mac.as_str()));
        }
        None
    }

    pub fn resolve(snapshot: &WiFiSnapshot, settings: &AppSettings) -> LockPolicy {
        if !snapshot.is_connected() {
            return LockPolicy::new(settings.no_wifi_timeout_seconds, PolicySource::NoWiFi);
        }
        if let Some(rule) = Self::matching_rule(snapshot, &settings.rules) {
            return LockPolicy::new(rule.timeout_seconds, PolicySource::Rule { rule: rule.clone() });
        }
        LockPolicy::new(settings.unknown_wifi_timeout_seconds, PolicySource::UnknownWiFi)
    }

    /// 현재 네트워크를 규칙으로 만든다. 연결되어 있지 않거나 식별할 정보가 없으면 None.
    pub fn make_rule(snapshot: &WiFiSnapshot, timeout_seconds: Option<i64>) -> Option<NetworkRule> {
        if !snapshot.is_connected() || (snapshot.ssid.is_none() && snapshot.router_mac.is_none()) {
            return None;
        }
        let name = match (&snapshot.ssid, &snapshot.router_mac) {
            (Some(s), _) => s.clone(),
            (None, Some(m)) => format!("공유기 {m}"),
            (None, None) => unreachable!(),
        };
        Some(NetworkRule::new(
            name,
            snapshot.ssid.as_deref(),
            snapshot.router_mac.as_deref(),
            timeout_seconds.unwrap_or(DEFAULT_TRUSTED_TIMEOUT),
        ))
    }
}
