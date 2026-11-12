use crate::policy::{LockPolicy, PolicyResolver, WiFiSnapshot};
use crate::settings::AppSettings;
use crate::Timestamp;

/// 와이파이가 잠깐 끊겼다 붙는 경우 짧은 잠금 시간이 바로 적용되지 않도록 한다.
///
/// - 바뀐 상태가 지금보다 **더 짧은(엄격한)** 잠금 시간을 뜻하면 `downgrade_delay` 초 동안 그대로 유지될 때만 받아들인다.
/// - 더 길거나 같은 잠금 시간이면 바로 받아들인다.
/// - 설정 변경은 늘 바로 반영된다 (매번 현재 설정으로 다시 계산하므로).
pub struct PolicyStabilizer {
    pub downgrade_delay: f64,
    stable: Option<WiFiSnapshot>,
    pending: Option<(LockPolicy, Timestamp)>,
}

impl PolicyStabilizer {
    pub fn new(downgrade_delay: f64) -> Self {
        PolicyStabilizer { downgrade_delay, stable: None, pending: None }
    }

    /// 지금 잠금 시간을 정할 때 쓰는 와이파이 상태
    pub fn stable(&self) -> Option<&WiFiSnapshot> {
        self.stable.as_ref()
    }

    /// 확인 중인 변화가 있으면 그 시작 시각
    pub fn pending_since(&self) -> Option<Timestamp> {
        self.pending.as_ref().map(|p| p.1)
    }

    /// 잠금 시간을 정할 때 쓸 와이파이 상태를 돌려준다.
    pub fn update(&mut self, snapshot: &WiFiSnapshot, settings: &AppSettings, now: Timestamp) -> WiFiSnapshot {
        let Some(current) = self.stable.clone() else {
            self.stable = Some(snapshot.clone());
            return snapshot.clone();
        };
        if *snapshot == current {
            self.pending = None;
            return current;
        }
        let new_policy = PolicyResolver::resolve(snapshot, settings);
        let old_policy = PolicyResolver::resolve(&current, settings);
        if !Self::is_stricter(&new_policy, &old_policy) {
            self.stable = Some(snapshot.clone());
            self.pending = None;
            return snapshot.clone();
        }
        match &self.pending {
            Some((p, since)) if *p == new_policy => {
                let since = *since;
                if now - since >= self.downgrade_delay {
                    self.stable = Some(snapshot.clone());
                    self.pending = None;
                    return snapshot.clone();
                }
            }
            _ => self.pending = Some((new_policy, now)),
        }
        current
    }

    /// a 가 b 보다 더 빨리 잠그는지
    pub fn is_stricter(a: &LockPolicy, b: &LockPolicy) -> bool {
        if a.never_locks() {
            return false;
        }
        if b.never_locks() {
            return true;
        }
        a.timeout_seconds < b.timeout_seconds
    }
}
