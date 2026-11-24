//! 맥 블루투스 구현: CoreBluetooth(BLE 스캔·MiBeacon MAC 추출) + IOBluetooth(페어링 기기·연결 상태·RSSI)

use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use objc2::rc::{autoreleasepool, Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass};
use objc2_foundation::{NSObject, NSString};
use wifilock_core::{
    infer_device_kind, mac_normalize, parse_mibeacon_mac, BluetoothDeviceKind, BluetoothObservation,
};

use crate::bt_common::merge_observations;
use crate::{BluetoothScanMode, BluetoothStatus};

#[link(name = "CoreBluetooth", kind = "framework")]
extern "C" {}
#[link(name = "IOBluetooth", kind = "framework")]
extern "C" {}

extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> *mut AnyObject;
}

const BLE_CACHE_TTL: f64 = 30.0;
const BLE_CACHE_MAX: usize = 512;
const PROFILER_CACHE_TTL: f64 = 20.0;

#[derive(Debug, Clone)]
struct ProfilerReading {
    connected: bool,
    rssi: Option<i16>,
    seen_at: f64,
}

#[derive(Default)]
struct BtShared {
    /// CBManagerState: 0=Unknown, 1=Resetting, 2=Unsupported, 3=Unauthorized, 4=PoweredOff, 5=PoweredOn
    cb_state: isize,
    ble_cache: HashMap<String, BluetoothObservation>,
    profiler_cache: HashMap<String, ProfilerReading>,
}

struct ScanControl {
    mode: BluetoothScanMode,
    init_requested: bool,
    wake_flag: bool,
}

#[derive(Clone)]
struct DelegateIvars {
    shared: Arc<Mutex<BtShared>>,
    ctrl: Arc<(Mutex<ScanControl>, Condvar)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "WiFiLockBtDelegate"]
    #[ivars = DelegateIvars]
    struct WiFiLockBtDelegate;

    impl WiFiLockBtDelegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn did_update_state(&self, central: *mut AnyObject) {
            if central.is_null() {
                return;
            }
            let state: isize = unsafe { msg_send![central, state] };
            {
                let mut g = self.ivars().shared.lock().unwrap_or_else(|e| e.into_inner());
                g.cb_state = state;
            }
            let (m, cv) = &*self.ivars().ctrl;
            if let Ok(mut c) = m.lock() {
                c.wake_flag = true;
                cv.notify_all();
            }
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn did_discover(
            &self,
            _central: *mut AnyObject,
            peripheral: *mut AnyObject,
            adv: *mut AnyObject,
            rssi: *mut AnyObject,
        ) {
            if peripheral.is_null() {
                return;
            }
            autoreleasepool(|_| unsafe {
                let Some(obs) = parse_discovered_peripheral(peripheral, adv, rssi) else {
                    return;
                };
                let mut g = self.ivars().shared.lock().unwrap_or_else(|e| e.into_inner());
                let now = obs.seen_at;
                if let Some(prev) = g.ble_cache.get_mut(&obs.key) {
                    if obs.name.is_some() {
                        prev.name = obs.name;
                    }
                    if obs.kind != BluetoothDeviceKind::Other {
                        prev.kind = obs.kind;
                    }
                    if obs.mac_address.is_some() {
                        prev.mac_address = obs.mac_address;
                    }
                    if obs.rssi.is_some() {
                        prev.rssi = obs.rssi;
                    }
                    prev.seen_at = now;
                } else {
                    g.ble_cache.insert(obs.key.clone(), obs);
                }
                if g.ble_cache.len() > BLE_CACHE_MAX {
                    g.ble_cache.retain(|_, v| now - v.seen_at <= BLE_CACHE_TTL);
                }
            });
        }
    }
);

unsafe fn is_kind_of(obj: *mut AnyObject, cls_name: &CStr) -> bool {
    if obj.is_null() {
        return false;
    }
    let Some(cls) = AnyClass::get(cls_name) else {
        return false;
    };
    msg_send![obj, isKindOfClass: cls]
}

unsafe fn nsstring_opt(obj: *mut AnyObject) -> Option<String> {
    if !is_kind_of(obj, c"NSString") {
        return None;
    }
    let s: Option<Retained<NSString>> = msg_send![obj, copy];
    s.map(|v| v.to_string()).filter(|s| !s.trim().is_empty())
}

unsafe fn nsdata_bytes<'a>(obj: *mut AnyObject) -> Option<&'a [u8]> {
    if !is_kind_of(obj, c"NSData") {
        return None;
    }
    let len: usize = msg_send![obj, length];
    if len == 0 {
        return Some(&[]);
    }
    let ptr: *const c_void = msg_send![obj, bytes];
    if ptr.is_null() {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr as *const u8, len))
}

unsafe fn parse_discovered_peripheral(
    peripheral: *mut AnyObject,
    adv: *mut AnyObject,
    rssi_obj: *mut AnyObject,
) -> Option<BluetoothObservation> {
    let ident: *mut AnyObject = msg_send![peripheral, identifier];
    if ident.is_null() {
        return None;
    }
    let uuid_str: Option<Retained<NSString>> = msg_send![ident, UUIDString];
    let uuid = uuid_str?.to_string();
    if uuid.is_empty() {
        return None;
    }

    let pname: Option<Retained<NSString>> = msg_send![peripheral, name];
    let mut name = pname.map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
    let mut mac_address: Option<String> = None;
    let mut is_mibeacon = false;

    if is_kind_of(adv, c"NSDictionary") {
        if name.is_none() {
            let key = NSString::from_str("kCBAdvDataLocalName");
            let val: *mut AnyObject = msg_send![adv, objectForKey: &*key];
            name = nsstring_opt(val);
        }
        let svc_key = NSString::from_str("kCBAdvDataServiceData");
        let svc_dict: *mut AnyObject = msg_send![adv, objectForKey: &*svc_key];
        if is_kind_of(svc_dict, c"NSDictionary") {
            let keys: *mut AnyObject = msg_send![svc_dict, allKeys];
            if is_kind_of(keys, c"NSArray") {
                let count: usize = msg_send![keys, count];
                for i in 0..count {
                    let k: *mut AnyObject = msg_send![keys, objectAtIndex: i];
                    if k.is_null() {
                        continue;
                    }
                    let u: Option<Retained<NSString>> = msg_send![k, UUIDString];
                    if u.as_deref().map(|s| s.to_string()).as_deref() == Some("FE95") {
                        is_mibeacon = true;
                        let val: *mut AnyObject = msg_send![svc_dict, objectForKey: k];
                        if let Some(bytes) = nsdata_bytes(val) {
                            mac_address = parse_mibeacon_mac(bytes);
                        }
                    }
                }
            }
        }
    }

    let rssi = if !rssi_obj.is_null() && is_kind_of(rssi_obj, c"NSNumber") {
        let raw: i16 = msg_send![rssi_obj, shortValue];
        BluetoothObservation::normalize_rssi(raw)
    } else {
        None
    };

    let kind = infer_device_kind(name.as_deref(), None, None, is_mibeacon, None);
    let now = crate::macos::clocks().total;

    Some(BluetoothObservation {
        key: format!("ble:{uuid}"),
        name,
        kind,
        ble_id: Some(uuid),
        mac_address,
        classic_address: None,
        rssi,
        connected: false,
        paired: false,
        seen_at: now,
    })
}

fn ns_dict_single(key: &str, val: bool) -> Option<Retained<AnyObject>> {
    unsafe {
        let num_cls = AnyClass::get(c"NSNumber")?;
        let dict_cls = AnyClass::get(c"NSDictionary")?;
        let num: *mut AnyObject = msg_send![num_cls, numberWithBool: val];
        let k = NSString::from_str(key);
        msg_send![dict_cls, dictionaryWithObject: num, forKey: &*k]
    }
}

struct CentralHolder {
    central: Retained<AnyObject>,
    _delegate: Retained<WiFiLockBtDelegate>,
}

impl CentralHolder {
    fn create(shared: Arc<Mutex<BtShared>>, ctrl: Arc<(Mutex<ScanControl>, Condvar)>) -> Option<Self> {
        autoreleasepool(|_| unsafe {
            let cls = AnyClass::get(c"CBCentralManager")?;
            let delegate_alloc = WiFiLockBtDelegate::alloc().set_ivars(DelegateIvars { shared, ctrl });
            let delegate: Retained<WiFiLockBtDelegate> = msg_send![super(delegate_alloc), init];
            let queue = dispatch_queue_create(c"com.wifilock.bt".as_ptr(), std::ptr::null());
            let opts = ns_dict_single("kCBInitOptionShowPowerAlert", false);
            let opts_ptr = opts.as_deref().map(|o| o as *const AnyObject).unwrap_or(std::ptr::null());
            let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
            let central: Option<Retained<AnyObject>> =
                msg_send![alloc, initWithDelegate: &*delegate, queue: queue, options: opts_ptr];
            central.map(|central| CentralHolder { central, _delegate: delegate })
        })
    }

    fn start_scan(&self) {
        autoreleasepool(|_| unsafe {
            let opts = ns_dict_single("kCBScanOptionAllowDuplicates", true);
            let opts_ptr = opts.as_deref().map(|o| o as *const AnyObject).unwrap_or(std::ptr::null());
            let null_services: *const AnyObject = std::ptr::null();
            let _: () = msg_send![&*self.central, scanForPeripheralsWithServices: null_services, options: opts_ptr];
        });
    }

    fn stop_scan(&self) {
        autoreleasepool(|_| unsafe {
            let _: () = msg_send![&*self.central, stopScan];
        });
    }
}

pub struct MacBluetooth {
    shared: Arc<Mutex<BtShared>>,
    ctrl: Arc<(Mutex<ScanControl>, Condvar)>,
}

impl MacBluetooth {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(BtShared::default()));
        let ctrl = Arc::new((
            Mutex::new(ScanControl {
                mode: BluetoothScanMode::Off,
                init_requested: false,
                wake_flag: false,
            }),
            Condvar::new(),
        ));

        let shared_w = shared.clone();
        let ctrl_w = ctrl.clone();
        let _ = std::thread::Builder::new()
            .name("wifilock-macos-bt".into())
            .spawn(move || worker_loop(shared_w, ctrl_w));

        MacBluetooth { shared, ctrl }
    }

    fn cb_authorization() -> Option<isize> {
        autoreleasepool(|_| unsafe {
            let cls = AnyClass::get(c"CBManager")?;
            let ok: bool = msg_send![cls, respondsToSelector: sel!(authorization)];
            if !ok {
                return None;
            }
            let auth: isize = msg_send![cls, authorization];
            Some(auth)
        })
    }

    fn host_controller_powered_on() -> Option<bool> {
        autoreleasepool(|_| unsafe {
            let cls = AnyClass::get(c"IOBluetoothHostController")?;
            let hc: *mut AnyObject = msg_send![cls, defaultController];
            if hc.is_null() {
                return None;
            }
            let ps: i32 = msg_send![hc, powerState];
            // IOBluetoothHCIControllerPowerState: 0 = Off, 1 = On
            Some(ps == 1)
        })
    }

    pub fn status(&self) -> BluetoothStatus {
        match Self::cb_authorization() {
            Some(0) => return BluetoothStatus::NotDetermined,
            Some(1 | 2) => return BluetoothStatus::Unauthorized,
            _ => {}
        }

        let cb_state = self.shared.lock().unwrap_or_else(|e| e.into_inner()).cb_state;
        match cb_state {
            2 => BluetoothStatus::Unavailable,
            3 => BluetoothStatus::Unauthorized,
            4 => BluetoothStatus::PoweredOff,
            5 => BluetoothStatus::Ready,
            _ => match Self::host_controller_powered_on() {
                Some(false) => BluetoothStatus::PoweredOff,
                Some(true) => BluetoothStatus::Ready,
                None => BluetoothStatus::Ready,
            },
        }
    }

    pub fn set_mode(&self, mode: BluetoothScanMode) {
        let (m, cv) = &*self.ctrl;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        if mode != BluetoothScanMode::Off {
            g.init_requested = true;
        }
        if g.mode != mode {
            g.mode = mode;
            g.wake_flag = true;
            cv.notify_all();
        }
    }

    pub fn request_permission(&self) {
        let status = self.status();
        if status == BluetoothStatus::Unauthorized {
            self.open_settings();
            return;
        }
        let (m, cv) = &*self.ctrl;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        g.init_requested = true;
        g.wake_flag = true;
        cv.notify_all();
    }

    pub fn open_settings(&self) {
        for url in [
            "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Bluetooth",
            "x-apple.systempreferences:com.apple.BluetoothSettings",
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Bluetooth",
        ] {
            if Command::new("/usr/bin/open").arg(url).status().map(|s| s.success()).unwrap_or(false) {
                return;
            }
        }
    }

    pub fn observations(&self) -> Vec<BluetoothObservation> {
        let now = crate::macos::clocks().total;
        let status = self.status();
        let (ble, profiler) = {
            let mut g = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            g.ble_cache.retain(|_, v| now - v.seen_at <= BLE_CACHE_TTL);
            g.profiler_cache.retain(|_, v| now - v.seen_at <= PROFILER_CACHE_TTL);
            (
                g.ble_cache.values().cloned().collect::<Vec<_>>(),
                g.profiler_cache.clone(),
            )
        };
        let paired = match status {
            BluetoothStatus::NotDetermined | BluetoothStatus::Unauthorized => Vec::new(),
            _ => {
                let mut list = read_iobluetooth_paired(now);
                for obs in &mut list {
                    if let Some(prof) = obs.classic_address.as_deref().and_then(|addr| profiler.get(addr)) {
                        if prof.connected {
                            obs.connected = true;
                            obs.seen_at = obs.seen_at.max(prof.seen_at);
                        }
                        if let Some(pr) = prof.rssi {
                            obs.rssi = Some(pr);
                            obs.seen_at = prof.seen_at;
                        }
                    }
                }
                list
            }
        };
        merge_observations(ble, paired)
    }
}

fn read_iobluetooth_paired(now: f64) -> Vec<BluetoothObservation> {
    autoreleasepool(|_| unsafe {
        let mut out = Vec::new();
        let Some(cls) = AnyClass::get(c"IOBluetoothDevice") else {
            return out;
        };
        let arr: *mut AnyObject = msg_send![cls, pairedDevices];
        if arr.is_null() {
            return out;
        }
        let count: usize = msg_send![arr, count];
        for i in 0..count {
            let dev: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            if dev.is_null() {
                continue;
            }
            let addr_ns: Option<Retained<NSString>> = msg_send![dev, addressString];
            let Some(addr) = addr_ns.and_then(|s| mac_normalize(&s.to_string())) else {
                continue;
            };
            let name_ns: Option<Retained<NSString>> = msg_send![dev, name];
            let name = name_ns.map(|s| s.to_string()).filter(|s| !s.trim().is_empty());
            let connected: bool = msg_send![dev, isConnected];
            // IOBluetoothDevice.rawRSSI()는 연결되지 않은 기기에 대해 오래된 캐시값을 고정 반환하므로
            // 실제로 연결된 상태일 때만 신뢰한다.
            let rssi = if connected {
                let raw_rssi: i8 = msg_send![dev, rawRSSI];
                BluetoothObservation::normalize_rssi(i16::from(raw_rssi))
            } else {
                None
            };
            let major: u32 = msg_send![dev, deviceClassMajor];
            let minor: u32 = msg_send![dev, deviceClassMinor];
            let kind = infer_device_kind(name.as_deref(), Some(major), Some(minor), false, None);

            out.push(BluetoothObservation {
                key: format!("paired:{addr}"),
                name,
                kind,
                ble_id: None,
                mac_address: None,
                classic_address: Some(addr),
                rssi,
                connected,
                paired: true,
                seen_at: if connected { now } else { 0.0 },
            });
        }
        out
    })
}

fn parse_profiler_rssi(val: &serde_json::Value) -> Option<i16> {
    if let Some(n) = val.as_i64() {
        i16::try_from(n).ok().and_then(BluetoothObservation::normalize_rssi)
    } else if let Some(s) = val.as_str() {
        s.trim().parse::<i16>().ok().and_then(BluetoothObservation::normalize_rssi)
    } else {
        None
    }
}

fn parse_system_profiler_bluetooth_json(json: &str, now: f64) -> HashMap<String, ProfilerReading> {
    let mut out = HashMap::new();
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return out;
    };
    let Some(controllers) = root.get("SPBluetoothDataType").and_then(|v| v.as_array()) else {
        return out;
    };
    for controller in controllers {
        for (field, connected) in [("device_not_connected", false), ("device_connected", true)] {
            let Some(entries) = controller.get(field).and_then(|v| v.as_array()) else {
                continue;
            };
            for entry in entries {
                let Some(obj) = entry.as_object() else {
                    continue;
                };
                for props in obj.values() {
                    let Some(addr) = props
                        .get("device_address")
                        .and_then(|v| v.as_str())
                        .and_then(mac_normalize)
                    else {
                        continue;
                    };
                    let rssi = props.get("device_rssi").and_then(parse_profiler_rssi);
                    if connected || rssi.is_some() {
                        out.insert(addr, ProfilerReading { connected, rssi, seen_at: now });
                    }
                }
            }
        }
    }
    out
}

fn query_system_profiler_bluetooth(now: f64) -> Option<HashMap<String, ProfilerReading>> {
    let out = Command::new("/usr/sbin/system_profiler")
        .args(["SPBluetoothDataType", "-json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    Some(parse_system_profiler_bluetooth_json(&text, now))
}

fn refresh_profiler_cache(shared: &Arc<Mutex<BtShared>>) {
    let now = crate::macos::clocks().total;
    if let Some(map) = query_system_profiler_bluetooth(now) {
        let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
        g.profiler_cache = map;
    }
}

fn wait_for_change_or_timeout(
    ctrl: &(Mutex<ScanControl>, Condvar),
    timeout: Option<Duration>,
) -> (BluetoothScanMode, bool) {
    let (m, cv) = ctrl;
    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
    if !g.wake_flag {
        match timeout {
            None => {
                while !g.wake_flag {
                    g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                }
            }
            Some(d) => {
                let deadline = Instant::now() + d;
                while !g.wake_flag {
                    let rem = deadline.saturating_duration_since(Instant::now());
                    if rem.is_zero() {
                        break;
                    }
                    let (ng, res) = cv.wait_timeout(g, rem).unwrap_or_else(|e| e.into_inner());
                    g = ng;
                    if res.timed_out() {
                        break;
                    }
                }
            }
        }
    }
    g.wake_flag = false;
    (g.mode, g.init_requested)
}

fn worker_loop(shared: Arc<Mutex<BtShared>>, ctrl: Arc<(Mutex<ScanControl>, Condvar)>) {
    let mut holder: Option<CentralHolder> = None;
    let mut scanning = false;

    loop {
        let (mode, init_req) = {
            let g = ctrl.0.lock().unwrap_or_else(|e| e.into_inner());
            (g.mode, g.init_requested)
        };

        if holder.is_none() && (init_req || mode != BluetoothScanMode::Off) {
            holder = CentralHolder::create(shared.clone(), ctrl.clone());
        }

        let powered_on = shared.lock().unwrap_or_else(|e| e.into_inner()).cb_state == 5;
        if !powered_on {
            if scanning {
                if let Some(h) = &holder {
                    h.stop_scan();
                }
                scanning = false;
            }
            let _ = wait_for_change_or_timeout(&ctrl, None);
            continue;
        }

        let Some(h) = holder.as_ref() else {
            let _ = wait_for_change_or_timeout(&ctrl, None);
            continue;
        };

        match mode {
            BluetoothScanMode::Off => {
                if scanning {
                    h.stop_scan();
                    scanning = false;
                }
                shared.lock().unwrap_or_else(|e| e.into_inner()).profiler_cache.clear();
                let _ = wait_for_change_or_timeout(&ctrl, None);
            }
            BluetoothScanMode::Radar => {
                if !scanning {
                    h.start_scan();
                    scanning = true;
                }
                refresh_profiler_cache(&shared);
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(2500)));
            }
            BluetoothScanMode::Background => {
                h.start_scan();
                scanning = true;
                refresh_profiler_cache(&shared);
                let (next_mode, _) = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(3000)));
                if next_mode != BluetoothScanMode::Background {
                    continue;
                }
                h.stop_scan();
                scanning = false;
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(4000)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_system_profiler_bluetooth_json;
    use crate::{create, BluetoothScanMode};
    use std::time::Duration;

    #[test]
    fn parses_system_profiler_bluetooth_json() {
        let sample = r#"{
          "SPBluetoothDataType": [
            {
              "device_connected": [
                {
                  "Heavyuser AirPods Pro": {
                    "device_address": "CC:22:FE:38:F9:C4",
                    "device_rssi": -54
                  }
                },
                {
                  "No Rssi Connected": {
                    "device_address": "11-22-33-44-55-66"
                  }
                }
              ],
              "device_not_connected": [
                {
                  "heavyuser": {
                    "device_address": "64:6D:2F:A7:28:FC",
                    "device_rssi": "-62"
                  }
                },
                {
                  "Invalid Rssi": {
                    "device_address": "AA:BB:CC:DD:EE:FF",
                    "device_rssi": "0"
                  }
                },
                {
                  "Keychron Q60 Max": {
                    "device_address": "E9:CC:6A:FD:81:C6"
                  }
                }
              ]
            }
          ]
        }"#;
        let parsed = parse_system_profiler_bluetooth_json(sample, 123.5);
        assert_eq!(parsed.len(), 3);

        let phone = parsed.get("64:6d:2f:a7:28:fc").expect("비연결 아이폰 RSSI 포함");
        assert!(!phone.connected);
        assert_eq!(phone.rssi, Some(-62));
        assert_eq!(phone.seen_at, 123.5);

        let airpods = parsed.get("cc:22:fe:38:f9:c4").expect("연결 에어팟 정수 RSSI 포함");
        assert!(airpods.connected);
        assert_eq!(airpods.rssi, Some(-54));

        let no_rssi_conn = parsed.get("11:22:33:44:55:66").expect("RSSI 없는 연결 기기도 포함");
        assert!(no_rssi_conn.connected);
        assert_eq!(no_rssi_conn.rssi, None);

        assert!(!parsed.contains_key("aa:bb:cc:dd:ee:ff"), "무효 RSSI(0) 비연결 기기는 제외");
        assert!(!parsed.contains_key("e9:cc:6a:fd:81:c6"), "RSSI 없는 비연결 기기는 제외");
    }

    #[test]
    #[ignore = "실제 macOS 블루투스 하드웨어 스모크 테스트"]
    fn live_macos_bluetooth_smoke() {
        let p = create();
        let st = p.bluetooth_status();
        println!("bluetooth_status={st:?}");
        p.set_bluetooth_mode(BluetoothScanMode::Radar);
        std::thread::sleep(Duration::from_secs(4));
        let obs = p.bluetooth_observations();
        println!("observations_count={}", obs.len());
        for o in obs.iter().filter(|o| o.paired || o.name.is_some()).take(20) {
            println!(
                "- {:?} | kind={:?} | rssi={:?} | conn={} | paired={} | ble={:?} | mac={:?} | classic={:?}",
                o.name, o.kind, o.rssi, o.connected, o.paired, o.ble_id, o.mac_address, o.classic_address
            );
        }
        p.set_bluetooth_mode(BluetoothScanMode::Off);
        assert!(!obs.is_empty(), "실제 macOS 블루투스 관측이 비어 있지 않아야 함");
    }
}
