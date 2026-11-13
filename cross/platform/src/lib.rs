//! 운영체제별 시스템 기능.
//!
//! 앱은 [`Platform`] 하나만 보고 동작하고, 맥·윈도우·리눅스의 차이는 이 크레이트 안에 가둔다.

use serde::Serialize;
use wifilock_core::WiFiSnapshot;

pub mod autolock;
pub mod parse;

pub use autolock::{SettingsTarget, SystemAutoLock, SystemLockNote};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// 와이파이 이름을 읽기 위한 위치 권한 상태
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LocationStatus {
    /// 이 운영체제에서는 필요 없음
    NotRequired,
    /// 아직 묻지 않음
    NotDetermined,
    /// 거절됨 (설정에서 허용해야 함)
    Denied,
    /// 관리 정책 등으로 막힘
    Restricted,
    /// 허용됨
    Authorized,
}

impl LocationStatus {
    pub fn has_permission(self) -> bool {
        matches!(self, LocationStatus::NotRequired | LocationStatus::Authorized)
    }
}

/// 운영체제별로 구현하는 시스템 기능
pub trait Platform: Send + Sync {
    /// "macos" / "windows" / "linux"
    fn os(&self) -> &'static str;

    /// 마지막 키보드·마우스 입력 후 지난 초. 읽을 수 없으면 None.
    fn idle_seconds(&self) -> Option<f64>;

    /// 화면이 잠겨 있거나, 이 사용자 세션이 지금 화면 앞에 있지 않은지 (빠른 사용자 전환 등)
    fn screen_locked(&self) -> bool;

    /// 화면 유지(화면 꺼짐 방지)를 요청한 앱 이름들. 이 앱 자신은 제외.
    fn display_sleep_preventers(&self) -> Vec<String>;

    /// 화면을 잠근다. 성공하면 사용한 방법을 돌려준다.
    /// 맥에서는 주 스레드에서 부르는 것이 안전하다.
    fn lock_screen(&self) -> Result<String, String>;

    /// 현재 와이파이 상태
    fn wifi_snapshot(&self) -> WiFiSnapshot;

    fn location_status(&self) -> LocationStatus;

    /// 위치 권한을 요청한다 (맥: 주 스레드에서 불러야 함). 요청 창을 띄울 수 없으면 설정 화면을 연다.
    fn request_location_permission(&self);

    /// 위치 권한 설정 화면을 연다.
    fn open_location_settings(&self);

    /// 운영체제가 스스로 화면을 잠그는 조건들 (짧은 것부터). 스스로 잠그지 않거나 알 수 없으면 비어 있다.
    /// 외부 명령을 부를 수 있어 느릴 수 있으므로 주 스레드에서 부르지 않는다.
    fn system_auto_locks(&self) -> Vec<SystemAutoLock>;

    /// 그 설정 화면을 연다.
    fn open_system_settings(&self, target: SettingsTarget);

    /// 자가 진단용: 기능별로 어떤 방법을 쓰는지
    fn diagnostics(&self) -> Vec<(String, String)>;
}

/// 단조 시계 두 개 (초). 벽시계와 달리 사용자가 시각을 바꿔도 흔들리지 않는다.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Clocks {
    /// 컴퓨터가 깨어 있던 시간만 센다 (잠자기 동안 멈춤)
    pub awake: f64,
    /// 잠자기 동안에도 흐른다
    pub total: f64,
}

/// 지금 단조 시계 값. 두 값의 차이가 늘어난 만큼 컴퓨터가 잠자기에 있었던 것이다.
pub fn clocks() -> Clocks {
    #[cfg(target_os = "macos")]
    {
        macos::clocks()
    }
    #[cfg(windows)]
    {
        windows::clocks()
    }
    #[cfg(target_os = "linux")]
    {
        linux::clocks()
    }
}

/// 이 운영체제용 구현을 만든다.
pub fn create() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacPlatform::new())
    }
    #[cfg(windows)]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
}

/// 맥에서 위치 권한 요청 창을 띄우려면 이 앱이 주 스레드에서 위치 관리자를 만들어 두어야 한다.
/// 다른 운영체제에서는 아무 일도 하지 않는다.
pub fn prepare_on_main_thread() {
    #[cfg(target_os = "macos")]
    macos::prepare_location_manager();
}

/// 자가 진단용: 이 프로세스가 화면 유지를 요청하거나 해제한다. 이 방법을 지원하면 true.
/// (맥은 `caffeinate -d`, 리눅스는 `systemd-inhibit` 같은 외부 명령으로 시험한다)
pub fn test_display_hold(on: bool) -> bool {
    #[cfg(windows)]
    {
        windows::test_display_hold(on);
        true
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        false
    }
}
