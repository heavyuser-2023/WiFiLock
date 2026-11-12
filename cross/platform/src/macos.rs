//! 맥 구현
//!
//! - 자리비움 시간: CoreGraphics `CGEventSourceSecondsSinceLastEventType`
//! - 잠김 여부: `CGSessionCopyCurrentDictionary` (잠김, 화면 앞 세션 여부)
//! - 화면 유지 요청: IOKit `IOPMCopyAssertionsByProcess`
//! - 잠그기: login.framework `SACLockScreenImmediate` (제어 센터·⌃⌘Q 와 같은 동작), 안 되면 디스플레이 끄기
//! - 와이파이: CoreWLAN (이름은 위치 권한 필요), 공유기 주소는 `ipconfig`·`arp` 로 시도

use std::ffi::c_void;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::{CFNumber, CFNumberRef};
use core_foundation::string::{CFString, CFStringRef};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::NSString;
use wifilock_core::{WiFiSnapshot, WiFiStatus};

use crate::parse;
use crate::{LocationStatus, Platform};

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(source_state: i32, event_type: u32) -> f64;
    fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMCopyAssertionsByProcess(assertions_by_pid: *mut CFDictionaryRef) -> i32;
}

// 클래스를 이름으로 찾을 수 있도록 프레임워크를 링크한다.
#[link(name = "CoreWLAN", kind = "framework")]
extern "C" {}
#[link(name = "CoreLocation", kind = "framework")]
extern "C" {}
#[link(name = "AppKit", kind = "framework")]
extern "C" {}

const HID_SYSTEM_STATE: i32 = 1;
const ANY_INPUT_EVENT: u32 = !0;

struct RouterCache {
    key: String,
    mac: Option<String>,
    checked_at: Option<Instant>,
    failures: u32,
}

type LockFn = unsafe extern "C" fn() -> i32;

pub struct MacPlatform {
    lock_fn: Option<LockFn>,
    _login: Option<libloading::Library>,
    router: Mutex<RouterCache>,
}

impl MacPlatform {
    pub fn new() -> Self {
        let path = "/System/Library/PrivateFrameworks/login.framework/Versions/Current/login";
        let (lib, f) = match unsafe { libloading::Library::new(path) } {
            Ok(lib) => {
                let f = unsafe { lib.get::<LockFn>(b"SACLockScreenImmediate\0").ok().map(|s| *s) };
                (Some(lib), f)
            }
            Err(_) => (None, None),
        };
        MacPlatform {
            lock_fn: f,
            _login: lib,
            router: Mutex::new(RouterCache { key: String::new(), mac: None, checked_at: None, failures: 0 }),
        }
    }

    fn session_value(key: &str) -> Option<bool> {
        unsafe {
            let raw = CGSessionCopyCurrentDictionary();
            if raw.is_null() {
                return None;
            }
            let dict: CFDictionary<CFString, CFType> = CFDictionary::wrap_under_create_rule(raw);
            let v = dict.find(CFString::new(key))?;
            if let Some(b) = v.downcast::<CFBoolean>() {
                return Some(b.into());
            }
            if let Some(n) = v.downcast::<CFNumber>() {
                return n.to_i64().map(|x| x != 0);
            }
            None
        }
    }

    fn running_app_name(pid: i32) -> Option<String> {
        autoreleasepool(|_| unsafe {
            let cls = AnyClass::get(c"NSRunningApplication")?;
            let app: Option<Retained<AnyObject>> = msg_send![cls, runningApplicationWithProcessIdentifier: pid];
            let app = app?;
            let name: Option<Retained<NSString>> = msg_send![&*app, localizedName];
            name.map(|n| n.to_string())
        })
    }

    /// 공유기 주소는 외부 명령으로 읽으므로 네트워크가 바뀌었거나 일정 시간이 지났을 때만 다시 읽는다.
    fn router_mac(&self, iface: &str, ssid: Option<&str>) -> Option<String> {
        let key = format!("{iface}|{}", ssid.unwrap_or(""));
        let mut c = self.router.lock().unwrap();
        if c.key != key {
            *c = RouterCache { key, mac: None, checked_at: None, failures: 0 };
        }
        // 실패하면 4초, 8초, 16초 … 최대 5분 간격으로 다시 시도 (macOS 가 주소를 가리는 환경 대비)
        let retry = Duration::from_secs((4u64 << c.failures.min(7)).min(300));
        let due = match c.checked_at {
            None => true,
            Some(t) if c.mac.is_none() => t.elapsed() > retry,
            Some(t) => t.elapsed() > Duration::from_secs(60),
        };
        if due {
            c.checked_at = Some(Instant::now());
            let found = lookup_router_mac(iface);
            c.failures = if found.is_some() { 0 } else { c.failures + 1 };
            if found.is_some() {
                c.mac = found;
            }
        }
        c.mac.clone()
    }

    fn sleep_display() -> Result<String, String> {
        Command::new("/usr/bin/pmset")
            .arg("displaysleepnow")
            .spawn()
            .map(|_| "디스플레이 끄기 (pmset displaysleepnow)".to_string())
            .map_err(|e| format!("디스플레이 끄기 실패: {e}"))
    }
}

fn lookup_router_mac(iface: &str) -> Option<String> {
    let out = Command::new("/usr/sbin/ipconfig").args(["getoption", iface, "router"]).output().ok()?;
    let ip = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if ip.is_empty() || ip.parse::<std::net::Ipv4Addr>().is_err() {
        return None;
    }
    let out = Command::new("/usr/sbin/arp").args(["-n", &ip]).output().ok()?;
    parse::arp_output_mac(&String::from_utf8_lossy(&out.stdout))
}

impl Platform for MacPlatform {
    fn os(&self) -> &'static str {
        "macos"
    }

    fn idle_seconds(&self) -> Option<f64> {
        let v = unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
        (v.is_finite() && v >= 0.0).then_some(v)
    }

    fn screen_locked(&self) -> bool {
        let locked = Self::session_value("CGSSessionScreenIsLocked").unwrap_or(false);
        let on_console = Self::session_value("kCGSSessionOnConsoleKey").unwrap_or(true);
        locked || !on_console
    }

    fn display_sleep_preventers(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let my_pid = std::process::id() as i64;
        unsafe {
            let mut raw: CFDictionaryRef = std::ptr::null();
            if IOPMCopyAssertionsByProcess(&mut raw) != 0 || raw.is_null() {
                return names;
            }
            let by_pid: CFDictionary = CFDictionary::wrap_under_create_rule(raw);
            let (keys, values) = by_pid.get_keys_and_values();
            let type_key = CFString::new("AssertType");
            let level_key = CFString::new("AssertLevel");
            let name_key = CFString::new("Process Name");
            for (k, v) in keys.iter().zip(values.iter()) {
                let pid = CFNumber::wrap_under_get_rule(*k as CFNumberRef).to_i64().unwrap_or(-1);
                if pid == my_pid {
                    continue;
                }
                let list: CFArray<CFDictionary<CFString, CFType>> = CFArray::wrap_under_get_rule(*v as CFArrayRef);
                let mut active = false;
                let mut proc_name: Option<String> = None;
                for a in list.iter() {
                    if proc_name.is_none() {
                        proc_name = a
                            .find(&name_key)
                            .and_then(|x| x.downcast::<CFString>())
                            .map(|s| s.to_string());
                    }
                    let ty = a.find(&type_key).and_then(|x| x.downcast::<CFString>()).map(|s| s.to_string());
                    let level = a.find(&level_key).and_then(|x| x.downcast::<CFNumber>()).and_then(|n| n.to_i64());
                    if matches!(ty.as_deref(), Some("PreventUserIdleDisplaySleep") | Some("NoDisplaySleepAssertion"))
                        && level != Some(0)
                    {
                        active = true;
                    }
                }
                if !active {
                    continue;
                }
                let name = Self::running_app_name(pid as i32).or(proc_name).unwrap_or_else(|| format!("PID {pid}"));
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
        names.sort();
        names
    }

    fn lock_screen(&self) -> Result<String, String> {
        if let Some(f) = self.lock_fn {
            let rc = unsafe { f() };
            if rc == 0 {
                // 2초 뒤에도 잠기지 않았다면 디스플레이를 끈다.
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_secs(2));
                    if !MacPlatform::session_value("CGSSessionScreenIsLocked").unwrap_or(false) {
                        let _ = MacPlatform::sleep_display();
                    }
                });
                return Ok("화면 잠금 (SACLockScreenImmediate)".into());
            }
            return Self::sleep_display().map(|m| format!("{m} — 잠금 함수 오류 {rc}"));
        }
        Self::sleep_display()
    }

    fn wifi_snapshot(&self) -> WiFiSnapshot {
        let read = autoreleasepool(|_| unsafe {
            let Some(cls) = AnyClass::get(c"CWWiFiClient") else {
                return (WiFiStatus::Unavailable, None, None);
            };
            let client: Option<Retained<AnyObject>> = msg_send![cls, sharedWiFiClient];
            let Some(client) = client else { return (WiFiStatus::Unavailable, None, None) };
            let iface: Option<Retained<AnyObject>> = msg_send![&*client, interface];
            let Some(iface) = iface else { return (WiFiStatus::Unavailable, None, None) };
            let name: Option<Retained<NSString>> = msg_send![&*iface, interfaceName];
            let name = name.map(|n| n.to_string());
            let power: bool = msg_send![&*iface, powerOn];
            if !power {
                return (WiFiStatus::PoweredOff, None, name);
            }
            let mode: isize = msg_send![&*iface, interfaceMode];
            let rssi: isize = msg_send![&*iface, rssiValue];
            if mode == 0 && rssi == 0 {
                return (WiFiStatus::Disconnected, None, name);
            }
            let ssid: Option<Retained<NSString>> = msg_send![&*iface, ssid];
            (WiFiStatus::Connected, ssid.map(|s| s.to_string()), name)
        });
        let (status, ssid, name) = read;
        let router = match (status, &name) {
            (WiFiStatus::Connected, Some(n)) => self.router_mac(n, ssid.as_deref()),
            _ => None,
        };
        WiFiSnapshot::new(status, ssid.as_deref(), router.as_deref(), name.as_deref())
    }

    fn location_status(&self) -> LocationStatus {
        autoreleasepool(|_| unsafe {
            let Some(cls) = AnyClass::get(c"CLLocationManager") else { return LocationStatus::Denied };
            let enabled: bool = msg_send![cls, locationServicesEnabled];
            if !enabled {
                return LocationStatus::Denied;
            }
            let status: i32 = msg_send![cls, authorizationStatus];
            match status {
                0 => LocationStatus::NotDetermined,
                1 => LocationStatus::Restricted,
                2 => LocationStatus::Denied,
                _ => LocationStatus::Authorized,
            }
        })
    }

    fn request_location_permission(&self) {
        if self.location_status() != LocationStatus::NotDetermined {
            self.open_location_settings();
            return;
        }
        prepare_location_manager();
        let mgr = LOCATION_MANAGER.lock().unwrap();
        if let Some(m) = mgr.as_ref() {
            unsafe {
                let _: () = msg_send![&*m.0, requestWhenInUseAuthorization];
            }
        }
    }

    fn open_location_settings(&self) {
        for url in [
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_LocationServices",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_LocationServices",
        ] {
            if Command::new("/usr/bin/open").arg(url).status().map(|s| s.success()).unwrap_or(false) {
                return;
            }
        }
    }

    fn diagnostics(&self) -> Vec<(String, String)> {
        vec![
            ("자리비움 시간".into(), "CGEventSourceSecondsSinceLastEventType".into()),
            (
                "화면 잠그기".into(),
                if self.lock_fn.is_some() { "SACLockScreenImmediate" } else { "pmset displaysleepnow (대체)" }.into(),
            ),
            ("잠김 여부".into(), "CGSessionCopyCurrentDictionary".into()),
            ("화면 유지 요청".into(), "IOPMCopyAssertionsByProcess".into()),
            ("와이파이".into(), "CoreWLAN (이름은 위치 권한 필요)".into()),
        ]
    }
}

/// 위치 관리자는 주 스레드에서 만들어 앱이 끝날 때까지 들고 있어야 요청 창이 뜬다.
struct ManagerHolder(Retained<AnyObject>);
// 주 스레드에서만 만들고 쓰며, 다른 스레드에서는 들고만 있는다.
unsafe impl Send for ManagerHolder {}

static LOCATION_MANAGER: Mutex<Option<ManagerHolder>> = Mutex::new(None);

pub fn prepare_location_manager() {
    let mut slot = LOCATION_MANAGER.lock().unwrap();
    if slot.is_some() {
        return;
    }
    unsafe {
        if let Some(cls) = AnyClass::get(c"CLLocationManager") {
            let m: Option<Retained<AnyObject>> = msg_send![cls, new];
            *slot = m.map(ManagerHolder);
        }
    }
}

#[allow(dead_code)]
fn _assert_types(_: CFStringRef, _: *const c_void) {}
