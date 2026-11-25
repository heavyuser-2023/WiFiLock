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
const PROFILER_LIVE_TTL: f64 = 25.0;
const PROFILER_CACHE_TTL: f64 = 60.0;

#[derive(Debug, Clone)]
struct ProfilerReading {
    connected: bool,
    rssi: Option<i16>,
    seen_at: f64,
}

#[derive(Debug, Clone)]
struct ProfilerTrack {
    connected: bool,
    rssi: Option<i16>,
    /// 연결 기기는 마지막 조회 시각, 비연결 기기는 RSSI 값이 실제로 변동된 마지막 시각
    changed_at: f64,
    /// 비연결 기기가 최소 1회 이상 RSSI 변동을 보여 실시간 신호임이 확인되었는지
    confirmed_live: bool,
    /// 마지막으로 system_profiler 응답에 등장한 시각
    polled_at: f64,
}

#[derive(Default)]
struct BtShared {
    /// CBManagerState: 0=Unknown, 1=Resetting, 2=Unsupported, 3=Unauthorized, 4=PoweredOff, 5=PoweredOn
    cb_state: isize,
    ble_cache: HashMap<String, BluetoothObservation>,
    profiler_tracks: HashMap<String, ProfilerTrack>,
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
            g.profiler_tracks.retain(|_, v| now - v.polled_at <= PROFILER_CACHE_TTL);
            (
                g.ble_cache.values().cloned().collect::<Vec<_>>(),
                g.profiler_tracks.clone(),
            )
        };
        let paired = match status {
            BluetoothStatus::NotDetermined | BluetoothStatus::Unauthorized => Vec::new(),
            _ => {
                let mut list = read_iobluetooth_paired(now);
                for obs in &mut list {
                    if let Some(track) = obs.classic_address.as_deref().and_then(|addr| profiler.get(addr)) {
                        if track.connected {
                            obs.connected = true;
                            obs.seen_at = obs.seen_at.max(track.changed_at);
                            if let Some(pr) = track.rssi {
                                obs.rssi = Some(pr);
                            }
                        } else if track.confirmed_live && now - track.changed_at <= PROFILER_LIVE_TTL {
                            if let Some(pr) = track.rssi {
                                obs.rssi = Some(pr);
                                obs.seen_at = obs.seen_at.max(track.changed_at);
                            }
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
                    if connected {
                        out.insert(addr, ProfilerReading { connected: true, rssi, seen_at: now });
                    } else if let Some(r) = rssi {
                        // macOS bluetoothd가 피어 통신을 잃어 펌웨어 0.0.0 · 제품 ID 없음으로 남은 항목은 죽은 캐시다.
                        let fw = props.get("device_firmwareVersion").and_then(|v| v.as_str()).unwrap_or("");
                        let has_product = props.get("device_productID").is_some();
                        if fw == "0.0.0" && !has_product {
                            continue;
                        }
                        out.insert(
                            addr,
                            ProfilerReading {
                                connected: false,
                                rssi: Some(r),
                                seen_at: now,
                            },
                        );
                    }
                }
            }
        }
    }
    out
}

fn update_profiler_tracks(
    tracks: &mut HashMap<String, ProfilerTrack>,
    fresh: HashMap<String, ProfilerReading>,
    now: f64,
) {
    for (addr, track) in tracks.iter_mut() {
        if !fresh.contains_key(addr) && track.connected {
            track.connected = false;
            track.confirmed_live = false;
        }
    }
    tracks.retain(|addr, track| fresh.contains_key(addr) || now - track.polled_at <= PROFILER_CACHE_TTL);

    for (addr, reading) in fresh {
        if reading.connected {
            tracks.insert(
                addr,
                ProfilerTrack {
                    connected: true,
                    rssi: reading.rssi,
                    changed_at: reading.seen_at,
                    confirmed_live: true,
                    polled_at: now,
                },
            );
            continue;
        }
        let Some(new_rssi) = reading.rssi else {
            continue;
        };
        match tracks.get_mut(&addr) {
            Some(prev) => {
                prev.connected = false;
                prev.polled_at = now;
                if prev.rssi != Some(new_rssi) {
                    prev.rssi = Some(new_rssi);
                    prev.changed_at = reading.seen_at;
                    prev.confirmed_live = true;
                } else if now - prev.changed_at > PROFILER_LIVE_TTL {
                    prev.confirmed_live = false;
                }
            }
            None => {
                // 비연결 기기의 첫 샘플은 고정 캐시일 수 있으므로 RSSI 변동이 관측된 뒤부터 실시간으로 인정한다.
                tracks.insert(
                    addr,
                    ProfilerTrack {
                        connected: false,
                        rssi: Some(new_rssi),
                        changed_at: reading.seen_at,
                        confirmed_live: false,
                        polled_at: now,
                    },
                );
            }
        }
    }
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
        update_profiler_tracks(&mut g.profiler_tracks, map, now);
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
                {
                    let mut g = shared.lock().unwrap_or_else(|e| e.into_inner());
                    for track in g.profiler_tracks.values_mut() {
                        track.connected = false;
                        track.confirmed_live = false;
                    }
                }
                let _ = wait_for_change_or_timeout(&ctrl, None);
            }
            BluetoothScanMode::Radar => {
                if !scanning {
                    h.start_scan();
                    scanning = true;
                }
                refresh_profiler_cache(&shared);
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(2000)));
            }
            BluetoothScanMode::Background => {
                if !scanning {
                    h.start_scan();
                    scanning = true;
                }
                refresh_profiler_cache(&shared);
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(4000)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_system_profiler_bluetooth_json, update_profiler_tracks, ProfilerReading, PROFILER_LIVE_TTL};
    use crate::{create, BluetoothScanMode};
    use std::collections::HashMap;
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
                    "device_firmwareVersion": "27.2.0",
                    "device_productID": "0x7504",
                    "device_rssi": "-55"
                  }
                },
                {
                  "Stale Off Phone": {
                    "device_address": "22:33:44:55:66:77",
                    "device_firmwareVersion": "0.0.0",
                    "device_rssi": "-61"
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

        let phone = parsed.get("64:6d:2f:a7:28:fc").expect("유효 펌웨어·제품ID 비연결 아이폰 포함");
        assert!(!phone.connected);
        assert_eq!(phone.rssi, Some(-55));
        assert_eq!(phone.seen_at, 123.5);

        let airpods = parsed.get("cc:22:fe:38:f9:c4").expect("연결 에어팟 정수 RSSI 포함");
        assert!(airpods.connected);
        assert_eq!(airpods.rssi, Some(-54));
        assert_eq!(airpods.seen_at, 123.5);

        let no_rssi_conn = parsed.get("11:22:33:44:55:66").expect("RSSI 없는 연결 기기도 포함");
        assert!(no_rssi_conn.connected);
        assert_eq!(no_rssi_conn.rssi, None);

        assert!(!parsed.contains_key("22:33:44:55:66:77"), "fw 0.0.0 + productID 없는 죽은 캐시 제외");
        assert!(!parsed.contains_key("aa:bb:cc:dd:ee:ff"), "무효 RSSI(0) 비연결 기기는 제외");
        assert!(!parsed.contains_key("e9:cc:6a:fd:81:c6"), "RSSI 없는 비연결 기기는 제외");
    }

    #[test]
    fn profiler_tracker_distinguishes_live_and_frozen_rssi() {
        let mut tracks = HashMap::new();
        let phone = "64:6d:2f:a7:28:fc".to_string();
        let pods = "cc:22:fe:38:f9:c4".to_string();

        let mk = |connected: bool, rssi: Option<i16>, seen_at: f64| ProfilerReading {
            connected,
            rssi,
            seen_at,
        };

        // 1. 처음 본 비연결 RSSI -61은 아직 변동이 없으므로 confirmed_live=false
        update_profiler_tracks(
            &mut tracks,
            HashMap::from([(phone.clone(), mk(false, Some(-61), 100.0)), (pods.clone(), mk(true, Some(-54), 100.0))]),
            100.0,
        );
        assert!(!tracks[&phone].confirmed_live);
        assert!(tracks[&pods].confirmed_live);

        // 2. 30초 뒤에도 -61로 고정되어 있으면 계속 confirmed_live=false
        update_profiler_tracks(
            &mut tracks,
            HashMap::from([(phone.clone(), mk(false, Some(-61), 130.0)), (pods.clone(), mk(true, Some(-54), 130.0))]),
            130.0,
        );
        assert!(!tracks[&phone].confirmed_live);
        assert!(tracks[&pods].confirmed_live, "연결된 기기는 RSSI가 고정돼도 계속 유효");

        // 3. RSSI가 -59 -> -55로 변하면 즉시 confirmed_live=true 및 changed_at 갱신
        update_profiler_tracks(&mut tracks, HashMap::from([(phone.clone(), mk(false, Some(-59), 134.0))]), 134.0);
        assert!(tracks[&phone].confirmed_live);
        assert_eq!(tracks[&phone].changed_at, 134.0);

        update_profiler_tracks(&mut tracks, HashMap::from([(phone.clone(), mk(false, Some(-55), 138.0))]), 138.0);
        assert!(tracks[&phone].confirmed_live);
        assert_eq!(tracks[&phone].changed_at, 138.0);

        // 4. 이후 -55로 동결되어 PROFILER_LIVE_TTL(25초)이 지나면 만료(confirmed_live=false)
        let expired_t = 138.0 + PROFILER_LIVE_TTL + 1.0;
        update_profiler_tracks(
            &mut tracks,
            HashMap::from([(phone.clone(), mk(false, Some(-55), expired_t))]),
            expired_t,
        );
        assert!(!tracks[&phone].confirmed_live);
    }

    #[test]
    #[ignore = "실제 macOS 블루투스 하드웨어 스모크 테스트"]
    fn live_macos_bluetooth_smoke() {
        let p = create();
        let st = p.bluetooth_status();
        println!("bluetooth_status={st:?}");
        p.set_bluetooth_mode(BluetoothScanMode::Radar);
        std::thread::sleep(Duration::from_secs(6));
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
