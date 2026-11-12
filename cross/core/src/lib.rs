//! 와이파이 잠금의 핵심 로직.
//!
//! 운영체제에 의존하지 않으므로 맥·윈도우·리눅스가 그대로 함께 쓴다.
//! 시스템 정보(자리비움 시간, 와이파이 상태 등)는 모두 바깥에서 넣어 주므로 가짜 값으로 시험할 수 있다.

pub mod controller;
pub mod format;
pub mod policy;
pub mod scheduler;
pub mod settings;
pub mod stabilizer;
pub mod store;

pub use controller::{Controller, LockStatus, PauseState, SystemInputs, TickResult};
pub use policy::{LockPolicy, PolicyResolver, PolicySource, WiFiSnapshot, WiFiStatus};
pub use scheduler::{EngineState, LockScheduler, TickInput, TickOutput};
pub use settings::{mac_normalize, AppSettings, NetworkRule, TimeoutPresets};
pub use stabilizer::PolicyStabilizer;
pub use store::SettingsStore;

/// 시각(초, 소수 포함). 판단에는 잠자기 동안에도 흐르는 단조 시계를 쓰고(부팅 기준),
/// 화면에 보여 줄 때만 벽시계(1970-01-01 UTC 기준)를 쓴다. 어느 쪽인지는 필드 설명을 따른다.
pub type Timestamp = f64;

/// 현재 벽시계 시각 (1970-01-01 UTC 기준)
pub fn now() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
