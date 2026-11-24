//! 윈도우 블루투스 구현: WinRT `BluetoothLEAdvertisementWatcher` + Win32 페어링 목록

use std::collections::HashMap;
use std::process::Command;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use wifilock_core::{
    infer_device_kind, parse_mibeacon_mac, BluetoothDeviceKind, BluetoothObservation,
};
use windows::core::Ref;
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEScanningMode,
};
use windows::Devices::Bluetooth::{BluetoothAdapter, BluetoothAddressType};
use windows::Devices::Radios::RadioState;
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;
use windows::Win32::Devices::Bluetooth::{
    BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice,
    BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS,
};

use crate::bt_common::{mac_from_bytes_rev, mac_from_u64_be, merge_observations};
use crate::{BluetoothScanMode, BluetoothStatus};

const BLE_CACHE_TTL: f64 = 30.0;
const BLE_CACHE_MAX: usize = 512;
const STATUS_CACHE_SECS: u64 = 5;

struct ScanControl {
    mode: BluetoothScanMode,
    wake_flag: bool,
}

pub struct WindowsBluetooth {
    ble_cache: Arc<Mutex<HashMap<String, BluetoothObservation>>>,
    ctrl: Arc<(Mutex<ScanControl>, Condvar)>,
    status_cache: Mutex<Option<(Instant, BluetoothStatus)>>,
}

impl WindowsBluetooth {
    pub fn new() -> Self {
        let ble_cache = Arc::new(Mutex::new(HashMap::new()));
        let ctrl = Arc::new((
            Mutex::new(ScanControl {
                mode: BluetoothScanMode::Off,
                wake_flag: false,
            }),
            Condvar::new(),
        ));

        let cache_w = ble_cache.clone();
        let ctrl_w = ctrl.clone();
        let _ = std::thread::Builder::new()
            .name("wifilock-win-bt".into())
            .spawn(move || worker_loop(cache_w, ctrl_w));

        WindowsBluetooth {
            ble_cache,
            ctrl,
            status_cache: Mutex::new(None),
        }
    }

    pub fn status(&self) -> BluetoothStatus {
        {
            let g = self.status_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((at, st)) = *g {
                if at.elapsed() < Duration::from_secs(STATUS_CACHE_SECS) {
                    return st;
                }
            }
        }
        let st = crate::windows::run_on_mta(query_adapter_status).unwrap_or(BluetoothStatus::Unavailable);
        *self.status_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), st));
        st
    }

    pub fn set_mode(&self, mode: BluetoothScanMode) {
        let (m, cv) = &*self.ctrl;
        let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
        if g.mode != mode {
            g.mode = mode;
            g.wake_flag = true;
            cv.notify_all();
        }
    }

    pub fn open_settings(&self) {
        let _ = Command::new("explorer.exe").arg("ms-settings:bluetooth").spawn();
    }

    pub fn observations(&self) -> Vec<BluetoothObservation> {
        let now = crate::windows::clocks().total;
        let ble = {
            let mut g = self.ble_cache.lock().unwrap_or_else(|e| e.into_inner());
            g.retain(|_, v| now - v.seen_at <= BLE_CACHE_TTL);
            g.values().cloned().collect::<Vec<_>>()
        };
        let paired = read_win32_paired(now);
        merge_observations(ble, paired)
    }
}

fn query_adapter_status() -> BluetoothStatus {
    let Ok(op) = BluetoothAdapter::GetDefaultAsync() else {
        return BluetoothStatus::Unavailable;
    };
    let Ok(adapter) = op.join() else {
        return BluetoothStatus::Unavailable;
    };
    if !adapter.IsLowEnergySupported().unwrap_or(true) {
        return BluetoothStatus::Unavailable;
    }
    let Ok(radio_op) = adapter.GetRadioAsync() else {
        return BluetoothStatus::Ready;
    };
    let Ok(radio) = radio_op.join() else {
        return BluetoothStatus::Ready;
    };
    match radio.State() {
        Ok(RadioState::On) => BluetoothStatus::Ready,
        Ok(RadioState::Off | RadioState::Disabled) => BluetoothStatus::PoweredOff,
        _ => BluetoothStatus::Unavailable,
    }
}

fn read_win32_paired(now: f64) -> Vec<BluetoothObservation> {
    let mut out = Vec::new();
    let params = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: true.into(),
        fReturnRemembered: true.into(),
        fReturnUnknown: false.into(),
        fReturnConnected: true.into(),
        fIssueInquiry: false.into(),
        cTimeoutMultiplier: 0,
        hRadio: Default::default(),
    };
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };

    unsafe {
        let Ok(find) = BluetoothFindFirstDevice(&params, &mut info) else {
            return out;
        };
        loop {
            let addr = mac_from_bytes_rev(&info.Address.Anonymous.rgBytes);
            if let Some(addr) = addr {
                let raw_name = crate::windows::wide_to_string(&info.szName);
                let name = if raw_name.trim().is_empty() { None } else { Some(raw_name) };
                let cod = info.ulClassofDevice;
                let major = (cod >> 8) & 0x1f;
                let minor = (cod >> 2) & 0x3f;
                let kind = infer_device_kind(name.as_deref(), Some(major), Some(minor), false, None);
                let connected = info.fConnected.as_bool();
                out.push(BluetoothObservation {
                    key: format!("paired:{addr}"),
                    name,
                    kind,
                    ble_id: None,
                    mac_address: None,
                    classic_address: Some(addr),
                    rssi: None,
                    connected,
                    paired: info.fAuthenticated.as_bool() || info.fRemembered.as_bool() || connected,
                    seen_at: if connected { now } else { 0.0 },
                });
            }
            info = BLUETOOTH_DEVICE_INFO {
                dwSize: std::mem::size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
                ..Default::default()
            };
            if BluetoothFindNextDevice(find, &mut info).is_err() {
                break;
            }
        }
        let _ = BluetoothFindDeviceClose(find);
    }
    out
}

fn parse_adv(args: &BluetoothLEAdvertisementReceivedEventArgs) -> Option<BluetoothObservation> {
    let raw_addr = args.BluetoothAddress().ok()?;
    let addr_mac = mac_from_u64_be(raw_addr)?;
    let addr_type = args.BluetoothAddressType().ok();
    let is_public = addr_type == Some(BluetoothAddressType::Public);

    let rssi = args
        .RawSignalStrengthInDBm()
        .ok()
        .and_then(BluetoothObservation::normalize_rssi);
    let adv = args.Advertisement().ok()?;
    let name = adv
        .LocalName()
        .ok()
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());

    let mut mibeacon_mac = None;
    let mut is_mibeacon = false;
    if let Ok(sections) = adv.DataSections() {
        let count = sections.Size().unwrap_or(0);
        for i in 0..count {
            let Ok(sec) = sections.GetAt(i) else { continue };
            if sec.DataType().ok() != Some(0x16) {
                continue;
            }
            let Ok(buf) = sec.Data() else { continue };
            let Ok(reader) = DataReader::FromBuffer(&buf) else { continue };
            let len = reader.UnconsumedBufferLength().unwrap_or(0) as usize;
            if len < 13 {
                continue;
            }
            let mut data = vec![0u8; len];
            if reader.ReadBytes(&mut data).is_ok() && data[0] == 0x95 && data[1] == 0xfe {
                is_mibeacon = true;
                mibeacon_mac = parse_mibeacon_mac(&data[2..]);
            }
        }
    }

    let mac_address = mibeacon_mac.or_else(|| if is_public { Some(addr_mac.clone()) } else { None });
    let kind = infer_device_kind(name.as_deref(), None, None, is_mibeacon, None);
    let now = crate::windows::clocks().total;

    Some(BluetoothObservation {
        key: format!("ble:{addr_mac}"),
        name,
        kind,
        ble_id: Some(format!("winle:{addr_mac}")),
        mac_address,
        classic_address: None,
        rssi,
        connected: false,
        paired: false,
        seen_at: now,
    })
}

fn create_watcher(cache: Arc<Mutex<HashMap<String, BluetoothObservation>>>) -> Option<(BluetoothLEAdvertisementWatcher, i64)> {
    let watcher = BluetoothLEAdvertisementWatcher::new().ok()?;
    let _ = watcher.SetScanningMode(BluetoothLEScanningMode::Active);
    let handler = TypedEventHandler::new(
        move |_sender: Ref<'_, BluetoothLEAdvertisementWatcher>,
              args: Ref<'_, BluetoothLEAdvertisementReceivedEventArgs>| {
            if let Some(args) = args.as_ref() {
                if let Some(obs) = parse_adv(args) {
                    let mut g = cache.lock().unwrap_or_else(|e| e.into_inner());
                    let now = obs.seen_at;
                    if let Some(prev) = g.get_mut(&obs.key) {
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
                        g.insert(obs.key.clone(), obs);
                    }
                    if g.len() > BLE_CACHE_MAX {
                        g.retain(|_, v| now - v.seen_at <= BLE_CACHE_TTL);
                    }
                }
            }
            Ok(())
        },
    );
    let token = watcher.Received(&handler).ok()?;
    Some((watcher, token))
}

fn wait_for_change_or_timeout(
    ctrl: &(Mutex<ScanControl>, Condvar),
    timeout: Option<Duration>,
) -> BluetoothScanMode {
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
    g.mode
}

fn worker_loop(cache: Arc<Mutex<HashMap<String, BluetoothObservation>>>, ctrl: Arc<(Mutex<ScanControl>, Condvar)>) {
    let mut watcher_pair: Option<(BluetoothLEAdvertisementWatcher, i64)> = None;
    let mut running = false;

    loop {
        let mode = ctrl.0.lock().unwrap_or_else(|e| e.into_inner()).mode;
        if mode != BluetoothScanMode::Off && watcher_pair.is_none() {
            watcher_pair = create_watcher(cache.clone());
        }

        let Some((w, _)) = watcher_pair.as_ref() else {
            let _ = wait_for_change_or_timeout(&ctrl, if mode == BluetoothScanMode::Off { None } else { Some(Duration::from_secs(10)) });
            continue;
        };

        match mode {
            BluetoothScanMode::Off => {
                if running {
                    let _ = w.Stop();
                    running = false;
                }
                let _ = wait_for_change_or_timeout(&ctrl, None);
            }
            BluetoothScanMode::Radar => {
                if !running {
                    running = w.Start().is_ok();
                }
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_secs(20)));
            }
            BluetoothScanMode::Background => {
                running = w.Start().is_ok();
                let next_mode = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(3000)));
                if next_mode != BluetoothScanMode::Background {
                    continue;
                }
                if running {
                    let _ = w.Stop();
                    running = false;
                }
                let _ = wait_for_change_or_timeout(&ctrl, Some(Duration::from_millis(4000)));
            }
        }
    }
}
