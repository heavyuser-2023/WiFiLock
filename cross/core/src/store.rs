use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use crate::policy::{PolicyResolver, WiFiSnapshot};
use crate::settings::{AppSettings, NetworkRule, DEFAULT_TRUSTED_TIMEOUT};

/// 설정을 JSON 파일로 저장·불러온다.
pub struct SettingsStore {
    path: Option<PathBuf>,
    settings: AppSettings,
}

impl SettingsStore {
    /// 파일에서 불러온다. 파일이 없거나 깨져 있으면 기본값.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let settings = Self::load(&path);
        SettingsStore { path: Some(path), settings }
    }

    /// 저장하지 않는 설정 (시험용)
    pub fn in_memory(settings: AppSettings) -> Self {
        SettingsStore { path: None, settings: settings.clamped() }
    }

    pub fn load(path: &Path) -> AppSettings {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<AppSettings>(&bytes).unwrap_or_default(),
            Err(_) => AppSettings::default(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// 설정을 바꾼다. 실제로 달라졌으면 저장하고 true.
    pub fn update(&mut self, f: impl FnOnce(&mut AppSettings)) -> bool {
        let mut next = self.settings.clone();
        f(&mut next);
        let next = next.clamped();
        if next == self.settings {
            return false;
        }
        self.settings = next;
        self.save();
        true
    }

    /// 통째로 바꾼다 (설정 창에서 보낸 값).
    pub fn replace(&mut self, settings: AppSettings) -> bool {
        self.update(|s| *s = settings)
    }

    /// 다른 파일에 쓰다 끊겨도 설정이 깨지지 않도록 임시 파일에 쓴 뒤 바꿔치기한다.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(json) = serde_json::to_vec_pretty(&self.settings) else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let ok = fs::File::create(&tmp)
            .and_then(|mut f| {
                f.write_all(&json)?;
                f.sync_all()
            })
            .is_ok();
        if ok {
            let _ = fs::rename(&tmp, path);
        } else {
            let _ = fs::remove_file(&tmp);
        }
    }

    /// 현재 네트워크를 규칙에 추가한다. 이미 같은 네트워크 규칙이 있으면 그 규칙을 돌려준다.
    pub fn add_rule_for(&mut self, snapshot: &WiFiSnapshot, timeout_seconds: Option<i64>) -> Option<NetworkRule> {
        if let Some(existing) = PolicyResolver::matching_rule(snapshot, &self.settings.rules).cloned() {
            // 이름을 몰랐던 규칙에 이제 이름을 알게 됐다면 채워 넣는다.
            if existing.ssid.is_none() && snapshot.ssid.is_some() && existing.router_mac == snapshot.router_mac {
                let ssid = snapshot.ssid.clone();
                let id = existing.id;
                self.update(|s| {
                    if let Some(r) = s.rules.iter_mut().find(|r| r.id == id) {
                        r.ssid = ssid;
                    }
                });
                return self.settings.rules.iter().find(|r| r.id == id).cloned();
            }
            return Some(existing);
        }
        let mut rule = PolicyResolver::make_rule(snapshot, Some(timeout_seconds.unwrap_or(DEFAULT_TRUSTED_TIMEOUT)))?;
        // 이름이 같은 다른 공유기의 네트워크가 이미 있으면 구분할 수 있게 공유기 주소 끝자리를 붙인다.
        if self.settings.rules.iter().any(|r| r.name == rule.name) {
            if let Some(mac) = &rule.router_mac {
                rule.name = format!("{} (공유기 …{})", rule.name, &mac[mac.len() - 5..]);
            }
        }
        let added = rule.clone();
        self.update(|s| s.rules.push(rule));
        Some(added)
    }

    /// 와이파이 이름을 직접 입력해 규칙을 추가한다. 빈 이름이거나 이미 있으면 None.
    pub fn add_rule_ssid(&mut self, raw_ssid: &str, timeout_seconds: i64) -> Option<NetworkRule> {
        let ssid = raw_ssid.trim();
        if ssid.is_empty() || self.settings.rules.iter().any(|r| r.ssid.as_deref() == Some(ssid)) {
            return None;
        }
        let rule = NetworkRule::new(ssid, Some(ssid), None, timeout_seconds);
        let added = rule.clone();
        self.update(|s| s.rules.push(rule));
        Some(added)
    }

    pub fn remove_rule(&mut self, id: Uuid) -> bool {
        self.update(|s| s.rules.retain(|r| r.id != id))
    }

    /// 규칙 순서를 바꾼다 (위에 있는 규칙이 먼저 적용).
    pub fn move_rule(&mut self, id: Uuid, up: bool) -> bool {
        self.update(|s| {
            if let Some(i) = s.rules.iter().position(|r| r.id == id) {
                let j = if up { i.checked_sub(1) } else { Some(i + 1).filter(|j| *j < s.rules.len()) };
                if let Some(j) = j {
                    s.rules.swap(i, j);
                }
            }
        })
    }
}
