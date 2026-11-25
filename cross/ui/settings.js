// 와이파이 잠금 설정 창
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let state = null;
let lastRulesKey = "";
let lastBtDevicesKey = "";
let lastRadar = null;
let radarOpen = false;
let appliedLang = "";
let reported = false;
let draftCustomRssi = null;
const blipMap = new Map();
/** 보낸 요청 번호와 아직 응답이 오지 않은 요청 수 (늦게 온 응답·알림이 새 값을 덮어쓰지 않게) */
let requestSeq = 0;
let inflight = 0;

const $ = (id) => document.getElementById(id);

const ICONS = {
  on: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0"/><path d="M5 12.6a10 10 0 0 1 14 0"/><path d="M8.5 16.3a5 5 0 0 1 7 0"/><circle cx="12" cy="19.6" r="1.1" fill="currentColor"/></svg>',
  off: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M8.5 16.3a5 5 0 0 1 7 0" opacity=".35"/><path d="M3 3l18 18"/></svg>',
  warn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M12 9v6"/><circle cx="12" cy="19.2" r="1.1" fill="currentColor"/></svg>',
};

const BT_ICONS = {
  laptop: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="5" width="16" height="11" rx="1.8"/><path d="M2 18.5h20"/></svg>',
  phone: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><rect x="7" y="2.8" width="10" height="18.4" rx="2.2"/><path d="M11 18h2"/></svg>',
  band: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3h6l.6 3.5H8.4z"/><rect x="7.5" y="6.5" width="9" height="11" rx="2.5"/><path d="M9 21h6l.6-3.5H8.4z"/></svg>',
  audio: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M4.5 13a7.5 7.5 0 0 1 15 0"/><rect x="3.5" y="13" width="3.5" height="6" rx="1.5"/><rect x="17" y="13" width="3.5" height="6" rx="1.5"/></svg>',
  other: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"><path d="M7 7l10 10-5 4V3l5 4L7 17"/></svg>',
};

const STRINGS = {
  ko: {
    title: "와이파이 잠금 설정",
    secGeneral: "일반",
    enabledLabel: "자동 잠금 사용",
    enabledDesc: "키보드·마우스 입력이 없으면 아래 시간 뒤에 화면을 잠급니다.",
    autostartLabel: "로그인할 때 자동으로 실행",
    secCurrent: "현재 상태",
    checking: "확인 중…",
    addCurrentInline: "등록",
    secRules: "네트워크별 잠금 시간",
    secHold: "잠금 보류",
    btEnabledLabel: "블루투스 기기가 근처에 있으면 잠그지 않기",
    btEnabledDesc: "등록된 기기가 선택한 거리 안에 있으면 잠금을 보류합니다.",
    btRangeLabel: "감지 거리",
    btRangeNear: "가까움 (약 1~2m)",
    btRangeMedium: "중간 가까움 (약 3~5m)",
    btRangeFar: "먼 거리 (약 6~10m)",
    btModeCustom: "사용자 지정",
    btResetPreset: "가까움으로",
    btScaleStrict: "더 가까이서만 (-45 dBm)",
    btScaleMax: "더 멀리까지 (-95 dBm)",
    btPresetShort: { near: "가까움", medium: "중간", far: "먼 거리" },
    btCustomDesc: (enter, exit) => `${enter} dBm 이상이면 잠금을 보류하고, ${exit} dBm 이하로 멀어지면 다시 잠급니다.`,
    btExitMeta: (exit) => `이탈 기준 ${exit} dBm`,
    btLiveSignal: (name, rssi) => `${name} · ${rssi} dBm`,
    btLiveWaiting: "등록된 기기 신호 대기 중",
    btSliderAria: "블루투스 근접 감도",
    btRadarToggleOpen: "주변 기기 레이더로 찾기…",
    btRadarToggleClose: "레이더 닫기",
    btRadarTitle: "주변 기기 레이더",
    btScanStart: "스캔 시작",
    btScanStop: "스캔 중지",
    btClose: "닫기",
    btScanning: "주변 기기 스캔 중",
    btIdle: "대기 중",
    btDiscovered: "발견된 기기",
    btEmptyDevices: "등록된 블루투스 기기가 없습니다. 아래 '주변 기기 레이더로 찾기'로 아이폰·미밴드·에어팟을 지정해 보세요.",
    btEmptyRadar: "표시할 기기를 아직 찾지 못했습니다. 기기 화면을 켜거나 가까이 가져와 보세요.",
    btLegReg: "등록됨",
    btLegIn: "범위 안",
    btLegOut: "범위 밖",
    btTagNear: "2m",
    btTagMedium: "5m",
    btTagFar: "10m",
    btRegister: "등록",
    btRegistered: "등록됨",
    btUnnamedPrefix: "이름 없는 기기",
    btShowUnnamed: (n) => `이름 없는 기기 포함 (${n}개 숨김)`,
    btHolding: "잠금 보류 중",
    btLeaving: (s) => `이탈 확인 (${s}초)`,
    btOutOfRange: "범위 밖",
    btNoSignal: "신호 없음",
    btConnected: "연결됨",
    btPaired: "페어링됨",
    btDisabledDevice: "꺼짐",
    btDeleteTip: "이 기기 삭제",
    btNameAria: "블루투스 기기 이름",
    btToggleAria: "이 기기 사용 여부",
    holdFooter:
      "등록된 블루투스 기기가 근처에 있거나 화면 유지 요청이 있는 동안에는 위의 네트워크 잠금 시간에 도달해도 잠금을 보류합니다. 운영체제 자체 잠금은 따로 동작합니다.",
    addCurrent: "＋ 현재 네트워크 추가",
    addByName: "이름으로 추가…",
    addFormLabel: "와이파이 이름으로 추가",
    addFormDesc: "대소문자까지 정확히 입력해 주세요.",
    newSsidPlaceholder: "와이파이 이름",
    addCancel: "취소",
    addCommit: "추가",
    defaultRuleLabel: "그 밖의 와이파이 · 연결 안 됨",
    defaultRuleDesc: "등록하지 않은 와이파이나 연결이 끊긴 상태에 적용됩니다.",
    rulesFooter:
      "위에 있는 규칙부터 적용됩니다. 해당 규칙이 없으면 위의 기본 잠금 시간을 따릅니다. 공유기 주소까지 기록된 규칙은 이름이 같아도 공유기가 다르면 적용하지 않습니다.",
    secAdvanced: "고급",
    warnLabel: "잠그기 전에 화면에 경고 표시",
    warningLabel: "경고 표시 시간",
    warningDesc:
      "잠금 시간이 짧으면 경고는 잠금 시간의 절반까지만 표시됩니다. 경고 중에 마우스나 키보드를 움직이면 취소됩니다.",
    respectLabel: "동영상 재생 중에는 잠그지 않기",
    respectDesc: "앱이 화면을 켜 두도록 요청한 동안(동영상, 발표, 화상 회의 등)은 잠금을 미룹니다.",
    countdownLabel: "메뉴 막대에 남은 시간 표시",
    countdownDesc:
      "분 단위는 남은 시간이 1분에 한 번만 바뀌고 마지막 1분만 초 단위로 세어, 매 초 바뀌는 초 단위보다 컴퓨터 자원을 덜 씁니다.",
    countdownModes: { off: "표시 안 함", minutes: "분 단위", seconds: "초 단위" },
    emptyRules:
      "등록된 네트워크가 없습니다. 집이나 사무실처럼 믿을 수 있는 네트워크를 등록하면 그곳에서는 잠금 시간을 길게 둘 수 있습니다.",
    currentRuleTip: "지금 연결된 네트워크",
    ruleNameAria: "네트워크 이름",
    lockTimeAria: "잠금 시간",
    wifiPrefix: "와이파이 이름: ",
    routerPrefix: "공유기: ",
    moveUpTip: "위로 (먼저 적용)",
    moveDownTip: "아래로",
    deleteText: "삭제",
    deleteTip: "이 네트워크 삭제",
    dryRunNote: " (시험 모드: 실제로 잠그지 않음)",
    enterWifiError: "와이파이 이름을 입력해 주세요.",
    secSuffix: "초",
    appName: "와이파이 잠금",
    osNames: { macos: "맥", windows: "윈도우", linux: "리눅스" },
  },
  en: {
    title: "WiFiLock Settings",
    secGeneral: "General",
    enabledLabel: "Enable Auto-Lock",
    enabledDesc: "Automatically locks the screen after the idle time below.",
    autostartLabel: "Launch at Login",
    secCurrent: "Current Status",
    checking: "Checking…",
    addCurrentInline: "Add",
    secRules: "Per-Network Lock Times",
    secHold: "Auto-Lock Hold",
    btEnabledLabel: "Hold when a Bluetooth device is nearby",
    btEnabledDesc: "Delays locking while any registered device is within the selected range.",
    btRangeLabel: "Detection Range",
    btRangeNear: "Near (~1–2m)",
    btRangeMedium: "Medium (~3–5m)",
    btRangeFar: "Far (~6–10m)",
    btModeCustom: "Custom",
    btResetPreset: "Reset to Near",
    btScaleStrict: "Tighter (-45 dBm)",
    btScaleMax: "Wider (-95 dBm)",
    btPresetShort: { near: "Near", medium: "Medium", far: "Far" },
    btCustomDesc: (enter, exit) => `Holds auto-lock at ${enter} dBm or stronger, and resumes below ${exit} dBm.`,
    btExitMeta: (exit) => `Leave below ${exit} dBm`,
    btLiveSignal: (name, rssi) => `${name} · ${rssi} dBm`,
    btLiveWaiting: "Waiting for registered device signal",
    btSliderAria: "Bluetooth proximity sensitivity",
    btRadarToggleOpen: "Scan Nearby Devices on Radar…",
    btRadarToggleClose: "Hide Radar",
    btRadarTitle: "Nearby Device Radar",
    btScanStart: "Start Scan",
    btScanStop: "Stop Scan",
    btClose: "Close",
    btScanning: "Scanning nearby",
    btIdle: "Standby",
    btDiscovered: "Discovered Devices",
    btEmptyDevices: "No Bluetooth devices added. Use the radar below to pick your phone, band, or earbuds.",
    btEmptyRadar: "No matching devices found yet. Wake your device or bring it closer.",
    btLegReg: "Added",
    btLegIn: "In range",
    btLegOut: "Out of range",
    btTagNear: "2m",
    btTagMedium: "5m",
    btTagFar: "10m",
    btRegister: "Add",
    btRegistered: "Added",
    btUnnamedPrefix: "Unnamed device",
    btShowUnnamed: (n) => `Show unnamed devices (${n} hidden)`,
    btHolding: "Holding lock",
    btLeaving: (s) => `Leaving (${s}s)`,
    btOutOfRange: "Out of range",
    btNoSignal: "No signal",
    btConnected: "Connected",
    btPaired: "Paired",
    btDisabledDevice: "Off",
    btDeleteTip: "Remove this device",
    btNameAria: "Bluetooth device name",
    btToggleAria: "Enable device",
    holdFooter:
      "Auto-lock is held while a nearby Bluetooth device or active wake lock is present, even after the lock timer expires. OS screen lock settings work separately.",
    addCurrent: "＋ Add Current Network",
    addByName: "Add by Name…",
    addFormLabel: "Add by Wi-Fi Name",
    addFormDesc: "Enter the exact case-sensitive Wi-Fi name.",
    newSsidPlaceholder: "Wi-Fi name",
    addCancel: "Cancel",
    addCommit: "Add",
    defaultRuleLabel: "Unregistered Wi-Fi · Not Connected",
    defaultRuleDesc: "Applies when connected to an unregistered Wi-Fi or offline.",
    rulesFooter:
      "Rules apply from top to bottom; if none match, the fallback timeout above is used. Router-bound rules only match that specific router.",
    secAdvanced: "Advanced",
    warnLabel: "Show warning before locking",
    warningLabel: "Warning duration",
    warningDesc:
      "For short lock times, the warning is capped at half the lock time. Moving the mouse or pressing a key cancels it.",
    respectLabel: "Do not lock during video playback",
    respectDesc: "Delays locking while apps keep the screen awake (videos, presentations, meetings, etc.).",
    countdownLabel: "Show countdown in menu bar",
    countdownDesc:
      "Minute mode updates once per minute until the final minute, using fewer resources than second-by-second updates.",
    countdownModes: { off: "Off", minutes: "Minutes", seconds: "Seconds" },
    emptyRules:
      "No registered networks. Add trusted networks like home or office to use longer lock times there.",
    currentRuleTip: "Currently connected network",
    ruleNameAria: "Network name",
    lockTimeAria: "Lock time",
    wifiPrefix: "Wi-Fi Name: ",
    routerPrefix: "Router: ",
    moveUpTip: "Move up (higher priority)",
    moveDownTip: "Move down",
    deleteText: "Delete",
    deleteTip: "Delete this network",
    dryRunNote: " (Dry-run mode: will not actually lock)",
    enterWifiError: "Please enter a Wi-Fi name.",
    secSuffix: " sec",
    appName: "WiFiLock",
    osNames: { macos: "macOS", windows: "Windows", linux: "Linux" },
  },
};

function initialLang() {
  const q = new URLSearchParams(window.location.search).get("lang");
  if (q === "ko" || q === "en") return q;
  return (navigator.language || "").toLowerCase().startsWith("ko") ? "ko" : "en";
}

function currentLang() {
  return (state && state.lang) || initialLang();
}

function t() {
  return STRINGS[currentLang()] || STRINGS.ko;
}

function applyStaticStrings(lang) {
  if (appliedLang === lang) return;
  appliedLang = lang;
  const s = STRINGS[lang] || STRINGS.ko;
  document.documentElement.lang = lang;
  document.title = s.title;

  const setSel = (selector, text) => {
    const el = document.querySelector(selector);
    if (el && el.textContent !== text) el.textContent = text;
  };
  setSel("#sec-general > h2", s.secGeneral);
  setText("enabled-label", s.enabledLabel);
  setText("enabled-desc", s.enabledDesc);
  setText("autostart-label", s.autostartLabel);

  setSel("#sec-current > h2", s.secCurrent);
  if (!state) setText("wifi-text", s.checking);
  setText("add-current-inline", s.addCurrentInline);

  setSel("#sec-rules > h2", s.secRules);
  setText("add-current", s.addCurrent);
  setText("add-by-name", s.addByName);
  setSel("#add-form .label", s.addFormLabel);
  setSel("#add-form .desc", s.addFormDesc);
  $("new-ssid").placeholder = s.newSsidPlaceholder;
  setText("add-cancel", s.addCancel);
  setText("add-commit", s.addCommit);
  setText("default-rule-label", s.defaultRuleLabel);
  setText("default-rule-desc", s.defaultRuleDesc);
  setText("rules-footer", s.rulesFooter);

  setSel("#sec-hold > h2", s.secHold);
  setText("bt-enabled-label", s.btEnabledLabel);
  setText("bt-enabled-desc", s.btEnabledDesc);
  setText("bt-range-label", s.btRangeLabel);
  setText("bt-range-legend", s.btRangeLabel);
  setText("bt-range-near", s.btRangeNear);
  setText("bt-range-medium", s.btRangeMedium);
  setText("bt-range-far", s.btRangeFar);
  setText("bt-rssi-reset", s.btResetPreset);
  setText("bt-scale-strict", s.btScaleStrict);
  setText("bt-scale-max", s.btScaleMax);
  $("bt-rssi-slider").setAttribute("aria-label", s.btSliderAria);
  setText("bt-radar-title", s.btRadarTitle);
  setText("bt-radar-close", s.btClose);
  setText("bt-radar-list-title", s.btDiscovered);
  setText("bt-leg-reg", s.btLegReg);
  setText("bt-leg-in", s.btLegIn);
  setText("bt-leg-out", s.btLegOut);
  setText("bt-tag-near", s.btTagNear);
  setText("bt-tag-medium", s.btTagMedium);
  setText("bt-tag-far", s.btTagFar);
  setText("respect-label", s.respectLabel);
  setText("respect-desc", s.respectDesc);
  setText("hold-footer", s.holdFooter);
  $("bt-radar-center").innerHTML = BT_ICONS.laptop;

  setSel("#sec-advanced > h2", s.secAdvanced);
  setText("warn-label", s.warnLabel);
  setText("warning-label", s.warningLabel);
  setText("warning-desc", s.warningDesc);
  setText("countdown-label", s.countdownLabel);
  setText("countdown-desc", s.countdownDesc);
}

function countdownMode(s) {
  if (!s.showCountdownInMenuBar) return "off";
  return s.countdownInMinutes ? "minutes" : "seconds";
}

function labelOf(value) {
  const found = state.timeoutLabels.find((c) => c.value === value);
  return found ? found.label : `${value}${t().secSuffix}`;
}

/** 현재 값이 목록에 없으면 끼워 넣는다. "잠그지 않음"(0)은 항상 마지막. */
function timeoutChoices(value) {
  const presets = state.timeoutPresets;
  if (presets.includes(value)) return presets;
  const list = presets.filter((v) => v !== 0);
  if (value > 0) list.push(value);
  list.sort((a, b) => a - b);
  list.push(0);
  return list;
}

function fillSelect(select, values, current, label) {
  const key = currentLang() + "|" + values.join(",") + "|" + current;
  if (select.dataset.key === key) return;
  select.dataset.key = key;
  select.replaceChildren(
    ...values.map((v) => {
      const o = document.createElement("option");
      o.value = String(v);
      o.textContent = label(v);
      return o;
    })
  );
  select.value = String(current);
}

function setText(id, text) {
  const e = $(id);
  if (e.textContent !== text) e.textContent = text;
}

function setChecked(id, value) {
  const e = $(id);
  if (e.checked !== value) e.checked = value;
}

/** 규칙 이름을 고치는 중인지 (그동안은 목록을 다시 그리지 않는다) */
function editingRuleName() {
  const a = document.activeElement;
  return !!a && a.classList.contains("name") && $("rules").contains(a);
}

function editingBtName() {
  const a = document.activeElement;
  return !!a && a.classList.contains("name") && $("bt-devices").contains(a);
}

async function call(cmd, args) {
  const seq = ++requestSeq;
  inflight++;
  try {
    const next = await invoke(cmd, args);
    // 그사이 다른 요청을 보냈다면 이 응답은 오래된 것이므로 그리지 않는다.
    if (next && seq === requestSeq) render(next);
    return next;
  } catch (e) {
    console.error(cmd, e);
    throw e;
  } finally {
    inflight--;
  }
}

/** 설정 일부를 바꿔 저장한다. */
function save(mutate) {
  const next = JSON.parse(JSON.stringify(state.settings));
  mutate(next);
  // 응답을 기다리지 않고 바로 반영해 둔다. 빠르게 연달아 바꿔도 앞의 변경이 뒤의 저장에 함께 실린다.
  state.settings = next;
  return call("save_settings", { settings: next });
}

function renderRules() {
  const s = state.settings;
  const str = t();
  const key = JSON.stringify([currentLang(), s.rules, state.currentRuleId, state.timeoutLabels]);
  const box = $("rules");
  // 이름을 고치는 중에만 다시 그리지 않는다 (입력이 끊기지 않도록).
  // 단추·선택 상자에 초점이 있어도 다시 그려야 삭제·순서 바꾸기가 바로 보인다 (윈도우·리눅스는 누른 단추로 초점이 옮겨 간다).
  if (key === lastRulesKey || editingRuleName()) return;
  lastRulesKey = key;

  if (s.rules.length === 0) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent = str.emptyRules;
    box.replaceChildren(p);
    return;
  }
  const rows = s.rules.map((rule, index) => {
    const row = document.createElement("div");
    row.className = "row rule";

    const mark = document.createElement("div");
    const current = rule.id === state.currentRuleId;
    mark.className = "mark" + (current ? " current" : "");
    mark.textContent = current ? "●" : "○";
    mark.title = current ? str.currentRuleTip : "";

    const text = document.createElement("div");
    text.className = "text";
    const name = document.createElement("input");
    name.type = "text";
    name.className = "name";
    name.value = rule.name;
    name.setAttribute("aria-label", str.ruleNameAria);
    name.addEventListener("change", () => {
      const v = name.value.trim();
      if (!v) {
        name.value = rule.name;
        return;
      }
      lastRulesKey = "";
      save((n) => {
        const r = n.rules.find((x) => x.id === rule.id);
        if (r) r.name = v;
      });
    });
    name.addEventListener("keydown", (e) => {
      if (e.key === "Enter") name.blur();
      if (e.key === "Escape") {
        name.value = rule.name;
        name.blur();
      }
    });
    const ident = document.createElement("div");
    ident.className = "desc";
    const parts = [];
    if (rule.ssid) parts.push(`${str.wifiPrefix}${rule.ssid}`);
    if (rule.routerMAC) parts.push(`${str.routerPrefix}${rule.routerMAC}`);
    ident.textContent = parts.join(" · ");
    text.append(name, ident);

    const select = document.createElement("select");
    select.setAttribute("aria-label", str.lockTimeAria);
    fillSelect(select, timeoutChoices(rule.timeoutSeconds), rule.timeoutSeconds, labelOf);
    select.addEventListener("change", () => {
      const v = Number(select.value);
      save((n) => {
        const r = n.rules.find((x) => x.id === rule.id);
        if (r) r.timeoutSeconds = v;
      });
    });

    const order = document.createElement("div");
    order.className = "order";
    const up = document.createElement("button");
    up.className = "icon";
    up.textContent = "↑";
    up.title = str.moveUpTip;
    up.disabled = index === 0;
    up.addEventListener("click", () => call("move_rule", { id: rule.id, up: true }));
    const down = document.createElement("button");
    down.className = "icon";
    down.textContent = "↓";
    down.title = str.moveDownTip;
    down.disabled = index === s.rules.length - 1;
    down.addEventListener("click", () => call("move_rule", { id: rule.id, up: false }));
    order.append(up, down);

    const del = document.createElement("button");
    del.className = "icon danger";
    del.textContent = str.deleteText;
    del.title = str.deleteTip;
    del.addEventListener("click", () => call("remove_rule", { id: rule.id }));

    row.append(mark, text, select, order, del);
    return row;
  });
  box.replaceChildren(...rows);
}

function btIdentText(dev, str) {
  const parts = [];
  if (dev.macAddress) parts.push(`MAC ${dev.macAddress}`);
  if (dev.classicAddress && dev.classicAddress !== dev.macAddress) {
    parts.push(`${str.btPaired} ${dev.classicAddress}`);
  }
  if (parts.length === 0 && dev.bleId) {
    parts.push(`BLE ${dev.bleId.slice(0, 8)}`);
  }
  return parts.join(" · ");
}

function btBadgeInfo(dev, live, str) {
  if (!dev.enabled) return { text: str.btDisabledDevice, cls: "" };
  if (!live) return { text: str.btNoSignal, cls: "" };
  const rssiStr = typeof live.rssi === "number" ? ` · ${live.rssi} dBm` : live.connected ? ` · ${str.btConnected}` : "";
  if (typeof live.leavingRemainingSeconds === "number") {
    const sec = Math.max(1, Math.ceil(live.leavingRemainingSeconds));
    return { text: `${str.btLeaving(sec)}${rssiStr}`, cls: "is-leaving" };
  }
  if (live.holding) {
    return { text: `${str.btHolding}${rssiStr}`, cls: "is-holding" };
  }
  if (typeof live.rssi === "number") {
    return { text: `${str.btOutOfRange}${rssiStr}`, cls: "" };
  }
  return { text: live.connected ? str.btConnected : str.btNoSignal, cls: "" };
}

function effectiveBtThresholds() {
  const s = state.settings;
  const bt = state.bluetooth;
  if (draftCustomRssi !== null) {
    return {
      enter: draftCustomRssi,
      exit: Math.max(-100, draftCustomRssi - 8),
      isCustom: true,
    };
  }
  if (typeof s.bluetoothCustomRssi === "number") {
    return {
      enter: s.bluetoothCustomRssi,
      exit: Math.max(-100, s.bluetoothCustomRssi - 8),
      isCustom: true,
    };
  }
  return {
    enter: typeof bt.enterRssi === "number" ? bt.enterRssi : -65,
    exit: typeof bt.exitRssi === "number" ? bt.exitRssi : -74,
    isCustom: false,
  };
}

function syncSliderVars(slider) {
  const min = Number(slider.min || 45);
  const max = Number(slider.max || 95);
  slider.style.setProperty("--attr-min", String(min));
  slider.style.setProperty("--attr-max", String(max));
  slider.style.setProperty("--control-value", String(slider.value));
}

function renderBluetoothTuner() {
  if (!state) return;
  const s = state.settings;
  const bt = state.bluetooth;
  const str = t();
  const rangeVal = s.bluetoothRange || "near";
  const rangeOpt = bt.rangeOptions.find((r) => r.value === rangeVal);
  const { enter, exit, isCustom } = effectiveBtThresholds();

  $("bt-range").classList.toggle("is-custom", isCustom);

  const readout = $("bt-rssi-readout");
  const modeLabel = isCustom ? str.btModeCustom : (str.btPresetShort[rangeVal] || str.btPresetShort.near);
  readout.textContent = `${modeLabel} · ${enter} dBm`;
  readout.classList.toggle("is-custom", isCustom);

  setText("bt-range-desc", isCustom ? str.btCustomDesc(enter, exit) : (rangeOpt ? rangeOpt.description : ""));
  $("bt-rssi-reset").hidden = !isCustom;

  for (const tick of document.querySelectorAll(".bt-slider-ticks .bt-tick")) {
    tick.classList.toggle("is-active", !isCustom && tick.dataset.preset === rangeVal);
  }

  const slider = $("bt-rssi-slider");
  const targetSliderVal = Math.max(45, Math.min(95, Math.round(-enter)));
  if (draftCustomRssi === null && Number(slider.value) !== targetSliderVal) {
    slider.value = String(targetSliderVal);
  }
  slider.setAttribute("aria-valuetext", `${modeLabel} ${enter} dBm`);
  syncSliderVars(slider);

  let bestLive = null;
  let bestDev = null;
  for (const dev of s.bluetoothDevices) {
    if (!dev.enabled) continue;
    const live = bt.live.find((l) => l.ruleId === dev.id);
    if (live && typeof live.rssi === "number") {
      if (!bestLive || live.rssi > bestLive.rssi) {
        bestLive = live;
        bestDev = dev;
      }
    }
  }

  const pin = $("bt-live-pin");
  const dot = $("bt-live-dot");
  let liveText = "";
  if (bestLive && bestDev) {
    const atten = Math.max(45, Math.min(95, -bestLive.rssi));
    const ratio = (atten - 45) / 50;
    const isHolding = !!bestLive.holding || bestLive.rssi >= enter;
    pin.style.setProperty("--live-ratio", ratio.toFixed(4));
    pin.classList.toggle("is-holding", isHolding);
    pin.hidden = false;
    dot.classList.toggle("is-holding", isHolding);
    liveText = str.btLiveSignal(bestDev.name, bestLive.rssi);
  } else {
    pin.hidden = true;
    dot.classList.toggle("is-holding", false);
    if (s.bluetoothDevices.length > 0) {
      liveText = str.btLiveWaiting;
    }
  }
  $("bt-live-meta").hidden = !liveText;
  setText("bt-live-text", liveText);
  setText("bt-exit-meta", str.btExitMeta(exit));
}

function renderBluetooth() {
  const s = state.settings;
  const bt = state.bluetooth;
  const str = t();

  setChecked("bt-enabled", s.bluetoothHoldEnabled);
  $("bt-range-row").classList.toggle("is-dimmed", !s.bluetoothHoldEnabled);
  $("bt-devices").classList.toggle("is-dimmed", !s.bluetoothHoldEnabled);

  const rangeVal = s.bluetoothRange || "near";
  for (const radio of document.querySelectorAll('input[name="bt-range"]')) {
    radio.checked = radio.value === rangeVal;
  }
  $("bt-radar-stage").dataset.range = rangeVal;
  const { enter } = effectiveBtThresholds();
  updateRadarZone(enter);
  renderBluetoothTuner();

  const notice = bt.notice;
  $("bt-notice").hidden = !notice;
  setText("bt-notice-title", notice ? notice.title : "");
  setText("bt-notice-expl", notice ? notice.explanation : "");
  $("bt-notice-btn").hidden = !(notice && notice.button);
  setText("bt-notice-btn", (notice && notice.button) || "");

  setText("bt-radar-toggle", radarOpen ? str.btRadarToggleClose : str.btRadarToggleOpen);

  // 등록 기기 목록 (구조가 바뀔 때만 행 DOM을 다시 만들고, 평상시 1초 갱신은 제자리 갱신해 스위치 조작 유실을 막는다)
  const box = $("bt-devices");
  const structKey = JSON.stringify([currentLang(), s.bluetoothDevices.map((d) => [d.id, d.kind])]);
  if (structKey !== lastBtDevicesKey) {
    lastBtDevicesKey = structKey;
    if (s.bluetoothDevices.length === 0) {
      const p = document.createElement("div");
      p.className = "empty";
      p.textContent = str.btEmptyDevices;
      box.replaceChildren(p);
    } else {
      const rows = s.bluetoothDevices.map((dev) => {
        const devId = dev.id;
        const latestName = () => {
          const cur = state && state.settings.bluetoothDevices.find((x) => x.id === devId);
          return cur ? cur.name : dev.name;
        };

        const row = document.createElement("div");
        row.className = "row bt-device";
        row.dataset.id = devId;

        const icon = document.createElement("div");
        icon.className = "bt-kind-icon";
        icon.innerHTML = BT_ICONS[dev.kind] || BT_ICONS.other;

        const text = document.createElement("div");
        text.className = "text";
        const name = document.createElement("input");
        name.type = "text";
        name.className = "name";
        name.setAttribute("aria-label", str.btNameAria);
        name.addEventListener("change", () => {
          const v = name.value.trim();
          if (!v) {
            name.value = latestName();
            return;
          }
          save((n) => {
            const d = n.bluetoothDevices.find((x) => x.id === devId);
            if (d) d.name = v;
          });
        });
        name.addEventListener("keydown", (e) => {
          if (e.key === "Enter") name.blur();
          if (e.key === "Escape") {
            name.value = latestName();
            name.blur();
          }
        });
        const ident = document.createElement("div");
        ident.className = "desc";
        text.append(name, ident);

        const badge = document.createElement("span");
        badge.className = "bt-badge";

        const toggle = document.createElement("input");
        toggle.type = "checkbox";
        toggle.className = "switch";
        toggle.setAttribute("aria-label", str.btToggleAria);
        toggle.addEventListener("change", () => {
          save((n) => {
            const d = n.bluetoothDevices.find((x) => x.id === devId);
            if (d) d.enabled = toggle.checked;
          });
        });

        const del = document.createElement("button");
        del.className = "icon danger";
        del.textContent = str.deleteText;
        del.title = str.btDeleteTip;
        del.addEventListener("click", () => removeBtDevice(devId));

        row._refs = { name, ident, badge, toggle };
        row.append(icon, text, badge, toggle, del);
        return row;
      });
      box.replaceChildren(...rows);
    }
  }

  for (const row of box.children) {
    const refs = row._refs;
    if (!refs) continue;
    const dev = s.bluetoothDevices.find((d) => d.id === row.dataset.id);
    if (!dev) continue;
    const live = bt.live.find((l) => l.ruleId === dev.id);
    const holding = !!(s.bluetoothHoldEnabled && live && live.holding);
    row.classList.toggle("is-holding", holding);
    if (document.activeElement !== refs.name && refs.name.value !== dev.name) {
      refs.name.value = dev.name;
    }
    const identText = btIdentText(dev, str);
    if (refs.ident.textContent !== identText) {
      refs.ident.textContent = identText;
    }
    const bi = btBadgeInfo(dev, live, str);
    const badgeClass = "bt-badge" + (bi.cls ? ` ${bi.cls}` : "");
    if (refs.badge.className !== badgeClass) {
      refs.badge.className = badgeClass;
    }
    if (refs.badge.textContent !== bi.text) {
      refs.badge.textContent = bi.text;
    }
    if (refs.toggle.checked !== dev.enabled) {
      refs.toggle.checked = dev.enabled;
    }
  }

  if (radarOpen && lastRadar) {
    renderRadar(lastRadar);
  }
}

function hashAngle(key) {
  let h = 2166136261;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return ((h >>> 0) % 360) * (Math.PI / 180);
}

function lerp(a, b, t) {
  return a + (b - a) * Math.max(0, Math.min(1, t));
}

function rssiToRadiusPct(rssi, connected, enterRssi) {
  if (typeof rssi === "number") {
    if (rssi >= -45) return 0.14;
    if (rssi >= -65) return lerp(0.34, 0.14, (rssi + 65) / 20);
    if (rssi >= -76) return lerp(0.64, 0.34, (rssi + 76) / 11);
    if (rssi >= -88) return lerp(0.92, 0.64, (rssi + 88) / 12);
    return lerp(0.97, 0.92, Math.max(0, (rssi + 100) / 12));
  }
  if (connected) {
    const th = typeof enterRssi === "number" ? enterRssi : -65;
    return Math.max(0.14, rssiToRadiusPct(th, false, th) * 0.72);
  }
  return null;
}

function updateRadarZone(enterRssi) {
  const pct = rssiToRadiusPct(enterRssi, false, enterRssi);
  const scale = pct === null ? 0.3696 : pct / 0.92;
  $("bt-radar-zone").style.setProperty("--zone-scale", scale.toFixed(4));
}

function rssiBars(rssi, connected) {
  if (typeof rssi === "number") {
    if (rssi >= -58) return 4;
    if (rssi >= -68) return 3;
    if (rssi >= -80) return 2;
    return 1;
  }
  return connected ? 3 : 0;
}

function isNamedOrImportant(d) {
  return !!d.registeredId || d.paired || (d.name && d.name.trim().length > 0);
}

function syncDeviceMembership(d) {
  if (d.registeredId) {
    return removeBtDevice(d.registeredId);
  }
  const str = t();
  const fallbackName = d.macAddress || d.classicAddress || (d.bleId ? `${str.btUnnamedPrefix} (${d.bleId.slice(0, 6)})` : str.btUnnamedPrefix);
  return addBtDevice({
    name: (d.name && d.name.trim()) || fallbackName,
    kind: d.kind || "other",
    bleId: d.bleId || null,
    macAddress: d.macAddress || null,
    classicAddress: d.classicAddress || null,
    enabled: true,
  });
}

async function addBtDevice(device) {
  const next = await call("bt_add_device", { device });
  if (radarOpen) {
    renderRadar(await invoke("bt_get_radar"));
  }
  return next;
}

async function removeBtDevice(id) {
  const next = await call("bt_remove_device", { id });
  if (radarOpen) {
    renderRadar(await invoke("bt_get_radar"));
  }
  return next;
}

function setHoverKey(key, on) {
  const blip = blipMap.get(key);
  if (blip) blip.classList.toggle("is-hovered", on);
  for (const row of document.querySelectorAll("#bt-radar-list .radar-item")) {
    if (row.dataset.key === key) row.classList.toggle("is-hovered", on);
  }
}

function renderRadar(snap) {
  if (!snap) return;
  lastRadar = snap;
  const str = t();
  const panel = $("bt-radar-panel");
  panel.classList.toggle("is-scanning", !!snap.scanning);
  setText("bt-radar-status", snap.scanning ? str.btScanning : str.btIdle);
  setText("bt-scan-btn", snap.scanning ? str.btScanStop : str.btScanStart);

  const range = (state && state.settings.bluetoothRange) || "near";
  $("bt-radar-stage").dataset.range = range;
  const { enter: enterThreshold, exit: exitThreshold } = state
    ? effectiveBtThresholds()
    : { enter: -65, exit: -74 };
  updateRadarZone(enterThreshold);

  const showUnnamed = $("bt-show-unnamed").checked;
  const all = snap.devices || [];
  const unnamedHidden = all.filter((d) => !isNamedOrImportant(d)).length;
  setText("bt-unnamed-label", str.btShowUnnamed(unnamedHidden));

  let visible = showUnnamed
    ? [...all.filter(isNamedOrImportant), ...all.filter((d) => !isNamedOrImportant(d)).slice(0, 15)]
    : all.filter(isNamedOrImportant);

  // 각 기기의 실시간 range 판정은 현재 선택된(또는 드래그 중인) 감도 기준과 맞춘다.
  visible = visible.map((d) => {
    const reg = state && state.settings.bluetoothDevices.find((r) =>
      (r.macAddress && (r.macAddress === d.macAddress || r.macAddress === d.classicAddress)) ||
      (r.classicAddress && (r.classicAddress === d.classicAddress || r.classicAddress === d.macAddress)) ||
      (r.bleId && d.bleId && r.bleId.toUpperCase() === d.bleId.toUpperCase())
    );
    let inRange;
    if (draftCustomRssi === null && typeof d.inRange === "boolean" && reg) {
      inRange = d.inRange;
    } else if (typeof d.rssi === "number") {
      inRange = d.connected ? d.rssi > exitThreshold : d.rssi >= enterThreshold;
    } else {
      inRange = !!d.connected;
    }
    return { ...d, registeredId: reg ? reg.id : d.registeredId, inRange };
  });

  setText("bt-radar-count", String(visible.length));

  // 블립 좌표 계산 (반지름 R=120px 기준, 인접 점 충돌 회피)
  const R = 120;
  const pts = [];
  for (const d of visible) {
    const pct = rssiToRadiusPct(d.rssi, d.connected, enterThreshold);
    if (pct === null) continue;
    pts.push({ d, r: pct * R, a: hashAngle(d.key) });
  }
  pts.sort((x, y) => x.a - y.a);
  for (let pass = 0; pass < 6; pass++) {
    for (let i = 0; i < pts.length; i++) {
      for (let j = i + 1; j < pts.length; j++) {
        const dx = pts[i].r * Math.cos(pts[i].a) - pts[j].r * Math.cos(pts[j].a);
        const dy = pts[i].r * Math.sin(pts[i].a) - pts[j].r * Math.sin(pts[j].a);
        if (Math.hypot(dx, dy) < 34) {
          pts[i].a -= 0.22;
          pts[j].a += 0.22;
        }
      }
    }
  }

  const blipsBox = $("bt-radar-blips");
  const activeKeys = new Set();
  for (const p of pts) {
    const d = p.d;
    activeKeys.add(d.key);
    let btn = blipMap.get(d.key);
    if (!btn) {
      btn = document.createElement("button");
      btn.type = "button";
      btn.className = "radar-blip";
      const node = document.createElement("span");
      node.className = "blip-node";
      const lbl = document.createElement("span");
      lbl.className = "blip-label";
      btn.append(node, lbl);
      btn.addEventListener("mouseenter", () => setHoverKey(d.key, true));
      btn.addEventListener("mouseleave", () => setHoverKey(d.key, false));
      btn.addEventListener("focus", () => setHoverKey(d.key, true));
      btn.addEventListener("blur", () => setHoverKey(d.key, false));
      blipMap.set(d.key, btn);
      blipsBox.appendChild(btn);
    }
    btn._device = d;
    btn.onclick = () => syncDeviceMembership(btn._device);

    const displayName = (d.name && d.name.trim()) || d.macAddress || d.classicAddress || str.btUnnamedPrefix;
    const node = btn.firstElementChild;
    const lbl = btn.lastElementChild;
    const kindKey = d.kind || "other";
    if (node.dataset.kind !== kindKey) {
      node.dataset.kind = kindKey;
      node.innerHTML = BT_ICONS[kindKey] || BT_ICONS.other;
    }
    if (lbl.textContent !== displayName) lbl.textContent = displayName;

    btn.classList.toggle("is-registered", !!d.registeredId);
    btn.classList.toggle("is-in-range", !!d.inRange);
    btn.classList.toggle("is-out", !d.registeredId && !d.inRange);

    const bx = Math.round(p.r * Math.cos(p.a));
    const by = Math.round(p.r * Math.sin(p.a));
    btn.style.setProperty("--bx", `${bx}px`);
    btn.style.setProperty("--by", `${by}px`);
    const rssiText = typeof d.rssi === "number" ? `${d.rssi} dBm` : d.connected ? str.btConnected : str.btOutOfRange;
    btn.title = `${displayName} (${rssiText})`;
  }
  for (const [k, btn] of blipMap.entries()) {
    if (!activeKeys.has(k)) {
      btn.remove();
      blipMap.delete(k);
    }
  }

  // 우측 발견 목록
  const listBox = $("bt-radar-list");
  if (visible.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty";
    empty.textContent = str.btEmptyRadar;
    listBox.replaceChildren(empty);
    return;
  }
  const items = visible.map((d) => {
    const row = document.createElement("div");
    row.className = "radar-item" + (d.registeredId ? " is-registered" : "");
    row.dataset.key = d.key;
    row.addEventListener("mouseenter", () => setHoverKey(d.key, true));
    row.addEventListener("mouseleave", () => setHoverKey(d.key, false));

    const icon = document.createElement("div");
    icon.className = "radar-item-icon";
    icon.innerHTML = BT_ICONS[d.kind] || BT_ICONS.other;

    const text = document.createElement("div");
    text.className = "radar-item-text";
    const nameEl = document.createElement("div");
    nameEl.className = "radar-item-name";
    nameEl.textContent = (d.name && d.name.trim()) || d.macAddress || d.classicAddress || `${str.btUnnamedPrefix} (${(d.bleId || "").slice(0, 6)})`;

    const meta = document.createElement("div");
    meta.className = "radar-item-meta";
    const bars = document.createElement("span");
    bars.className = "sig-bars";
    bars.dataset.level = String(rssiBars(d.rssi, d.connected));
    bars.innerHTML = "<i></i><i></i><i></i><i></i>";
    const tags = [];
    if (typeof d.rssi === "number") tags.push(`${d.rssi} dBm`);
    if (d.connected) tags.push(str.btConnected);
    else if (d.paired) tags.push(str.btPaired);
    if (tags.length === 0) tags.push(str.btNoSignal);
    const metaText = document.createElement("span");
    metaText.textContent = tags.join(" · ");
    meta.append(bars, metaText);

    text.append(nameEl, meta);

    const btn = document.createElement("button");
    btn.className = d.registeredId ? "is-added" : "";
    btn.textContent = d.registeredId ? `✓ ${str.btRegistered}` : str.btRegister;
    btn.addEventListener("click", () => syncDeviceMembership(d));

    row.append(icon, text, btn);
    return row;
  });
  listBox.replaceChildren(...items);
}

function render(next) {
  state = next;
  applyStaticStrings(currentLang());
  const str = t();
  const s = state.settings;

  setChecked("enabled", s.enabled);
  setChecked("autostart", state.autostart);
  const err = $("autostart-error");
  err.hidden = !state.autostartError;
  err.textContent = state.autostartError || "";

  // 현재 네트워크
  const wifi = state.wifi;
  const icon = $("wifi-icon");
  const kind = wifi.status === "connected" ? "on" : wifi.status === "disconnected" ? "warn" : "off";
  if (icon.dataset.kind !== kind) {
    icon.dataset.kind = kind;
    icon.innerHTML = ICONS[kind];
    icon.classList.toggle("on", kind === "on");
  }
  setText("wifi-text", state.wifiText);
  $("router-text").hidden = !state.routerText;
  setText("router-text", state.routerText || "");
  setText("policy-text", state.policyText);
  setText("state-text", state.stateText + (state.dryRun ? str.dryRunNote : ""));
  $("state-text").classList.toggle("holding", !!state.isHeld);
  $("add-current-inline").hidden = !state.canAddCurrent;
  $("add-current").disabled = !state.canAddCurrent;

  const loc = state.location;
  $("permission").hidden = !loc.needsPermission;
  setText("perm-title", loc.title);
  setText("perm-expl", loc.explanation);
  setText("perm-button", loc.button);

  // 운영체제가 이 앱보다 먼저 잠그는 경우
  const sl = state.systemLock;
  $("syslock").hidden = !sl;
  setText("syslock-title", sl ? sl.title : "");
  setText("syslock-expl", sl ? sl.explanation : "");
  $("syslock-button").hidden = !(sl && sl.button);
  setText("syslock-button", (sl && sl.button) || "");

  renderRules();
  renderBluetooth();

  // 기본 잠금 시간 (그 밖의 와이파이 · 연결 안 됨)
  fillSelect($("unknown"), timeoutChoices(s.unknownWiFiTimeoutSeconds), s.unknownWiFiTimeoutSeconds, labelOf);
  if (!$("new-timeout").dataset.key || $("new-timeout").dataset.lang !== currentLang()) {
    $("new-timeout").dataset.lang = currentLang();
    fillSelect($("new-timeout"), timeoutChoices(1800), Number($("new-timeout").value) || 1800, labelOf);
  }

  // 고급
  setChecked("warn", s.warnBeforeLock);
  fillSelect($("warning"), state.warningOptions.map((c) => c.value), s.warningSeconds, (v) => {
    const c = state.warningOptions.find((x) => x.value === v);
    return c ? c.label : `${v}${str.secSuffix}`;
  });
  $("warning").disabled = !s.warnBeforeLock;
  $("warning-row").classList.toggle("is-dimmed", !s.warnBeforeLock);
  setChecked("respect", s.respectDisplaySleepAssertions);
  $("holders-note").hidden = !state.holdersNote;
  setText("holders-note", state.holdersNote);
  $("countdown-row").hidden = !state.countdownSupported;
  fillSelect(
    $("countdown"),
    Object.keys(str.countdownModes),
    countdownMode(s),
    (v) => str.countdownModes[v]
  );

  const osName = str.osNames[state.os] || state.os;
  setText("about", `${str.appName} ${state.version} · ${osName}`);

  if (!reported) {
    reported = true;
    sendReport("load");
  }
}

/** 지금 화면에 보이는 내용을 앱에 알린다 (자가 진단 확인용). */
function sendReport(kind) {
  return invoke("page_report", {
    report: {
      kind,
      lang: currentLang(),
      title: document.title,
      sections: [...document.querySelectorAll("section > h2")].map((h) => h.textContent),
      ruleCount: document.querySelectorAll("#rules .rule").length,
      ruleNames: [...document.querySelectorAll("#rules .rule input.name")].map((i) => i.value),
      systemLock: { shown: !$("syslock").hidden, title: $("syslock-title").textContent },
      bluetooth: {
        radarOpen: !$("bt-radar-panel").hidden,
        radarBlips: document.querySelectorAll("#bt-radar-blips .radar-blip").length,
        range: state.settings.bluetoothRange,
        customRssi: state.settings.bluetoothCustomRssi ?? null,
      },
      os: state.os,
    },
  }).catch(() => {});
}

function bind() {
  $("enabled").addEventListener("change", (e) => save((n) => (n.enabled = e.target.checked)));
  $("autostart").addEventListener("change", (e) => call("set_autostart", { enabled: e.target.checked }));
  $("warn").addEventListener("change", (e) => save((n) => (n.warnBeforeLock = e.target.checked)));
  $("respect").addEventListener("change", (e) => save((n) => (n.respectDisplaySleepAssertions = e.target.checked)));
  $("countdown").addEventListener("change", (e) =>
    save((n) => {
      const mode = e.target.value;
      n.showCountdownInMenuBar = mode !== "off";
      // "표시 안 함"을 고르면 분·초 선택은 그대로 둔다 (다시 켤 때 전에 쓰던 단위로)
      if (mode !== "off") n.countdownInMinutes = mode === "minutes";
    })
  );
  $("unknown").addEventListener("change", (e) =>
    save((n) => {
      const v = Number(e.target.value);
      n.unknownWiFiTimeoutSeconds = v;
      n.noWiFiTimeoutSeconds = v;
    })
  );
  $("warning").addEventListener("change", (e) => save((n) => (n.warningSeconds = Number(e.target.value))));

  $("add-current").addEventListener("click", () => call("add_current_network"));
  $("add-current-inline").addEventListener("click", () => call("add_current_network"));
  $("perm-button").addEventListener("click", () => invoke("request_location"));
  $("syslock-button").addEventListener("click", () => invoke("open_system_lock_settings"));

  const form = $("add-form");
  const showError = (msg) => {
    $("add-error").hidden = !msg;
    $("add-error").textContent = msg || "";
  };
  $("add-by-name").addEventListener("click", () => {
    form.hidden = false;
    showError("");
    $("new-ssid").value = "";
    $("new-ssid").focus();
  });
  $("add-cancel").addEventListener("click", () => {
    form.hidden = true;
  });
  const commit = async () => {
    const ssid = $("new-ssid").value.trim();
    if (!ssid) {
      showError(t().enterWifiError);
      return;
    }
    try {
      await call("add_rule_by_name", { ssid, timeout: Number($("new-timeout").value) });
      form.hidden = true;
    } catch (e) {
      showError(String(e));
    }
  };
  $("add-commit").addEventListener("click", commit);
  $("new-ssid").addEventListener("keydown", (e) => {
    if (e.key === "Enter") commit();
    if (e.key === "Escape") form.hidden = true;
  });

  $("bt-enabled").addEventListener("change", (e) => save((n) => (n.bluetoothHoldEnabled = e.target.checked)));
  for (const radio of document.querySelectorAll('input[name="bt-range"]')) {
    radio.addEventListener("change", () => {
      if (radio.checked) {
        draftCustomRssi = null;
        save((n) => {
          n.bluetoothRange = radio.value;
          n.bluetoothCustomRssi = null;
        });
        if (radarOpen && lastRadar) renderRadar(lastRadar);
      }
    });
    radio.addEventListener("click", () => {
      if (state && typeof state.settings.bluetoothCustomRssi === "number" && radio.value === state.settings.bluetoothRange) {
        draftCustomRssi = null;
        save((n) => (n.bluetoothCustomRssi = null));
        if (radarOpen && lastRadar) renderRadar(lastRadar);
      }
    });
  }
  $("bt-rssi-slider").addEventListener("input", (e) => {
    draftCustomRssi = -Number(e.target.value);
    renderBluetoothTuner();
    updateRadarZone(draftCustomRssi);
    if (radarOpen && lastRadar) renderRadar(lastRadar);
  });
  $("bt-rssi-slider").addEventListener("change", (e) => {
    const val = -Number(e.target.value);
    draftCustomRssi = null;
    save((n) => (n.bluetoothCustomRssi = val));
  });
  $("bt-rssi-reset").addEventListener("click", () => {
    draftCustomRssi = null;
    save((n) => {
      n.bluetoothRange = "near";
      n.bluetoothCustomRssi = null;
    });
    if (radarOpen && lastRadar) renderRadar(lastRadar);
  });
  $("bt-notice-btn").addEventListener("click", async () => {
    const action = state && state.bluetooth.notice && state.bluetooth.notice.action;
    if (action === "permission") {
      await call("bt_request_permission");
    } else if (action === "settings") {
      await invoke("bt_open_settings");
    }
  });
  $("bt-radar-toggle").addEventListener("click", async () => {
    radarOpen = !radarOpen;
    $("bt-radar-panel").hidden = !radarOpen;
    setText("bt-radar-toggle", radarOpen ? t().btRadarToggleClose : t().btRadarToggleOpen);
    if (radarOpen) {
      renderRadar(await invoke("bt_start_scan"));
    } else {
      lastRadar = await invoke("bt_stop_scan");
      renderRadar(lastRadar);
    }
  });
  $("bt-radar-close").addEventListener("click", async () => {
    radarOpen = false;
    $("bt-radar-panel").hidden = true;
    setText("bt-radar-toggle", t().btRadarToggleOpen);
    lastRadar = await invoke("bt_stop_scan");
  });
  $("bt-scan-btn").addEventListener("click", async () => {
    const scanning = lastRadar && lastRadar.scanning;
    renderRadar(await invoke(scanning ? "bt_stop_scan" : "bt_start_scan"));
  });
  $("bt-show-unnamed").addEventListener("change", () => {
    if (lastRadar) renderRadar(lastRadar);
  });
}

// 자가 진단에서 화면 조작을 흉내 낼 때 쓴다.
window.wifilockTest = {
  setUnknown(value) {
    const sel = $("unknown");
    fillSelect(sel, timeoutChoices(value), value, labelOf);
    sel.value = String(value);
    sel.dispatchEvent(new Event("change"));
  },
  addByName(name, timeout) {
    return call("add_rule_by_name", { ssid: name, timeout });
  },
  /** 사용자가 누르는 것처럼 삭제 단추에 초점을 준 뒤 누른다. */
  deleteByName(name) {
    const row = [...document.querySelectorAll("#rules .rule")].find((r) => r.querySelector("input.name").value === name);
    if (!row) return;
    const btn = row.querySelector("button.danger");
    btn.focus();
    btn.click();
  },
  setBluetoothRange(range) {
    const radio = document.querySelector(`input[name="bt-range"][value="${range}"]`);
    if (!radio) return;
    radio.checked = true;
    radio.dispatchEvent(new Event("change"));
  },
  setBluetoothCustomRssi(rssi) {
    const slider = $("bt-rssi-slider");
    if (!slider) return;
    slider.value = String(Math.max(45, Math.min(95, Math.round(Math.abs(rssi)))));
    slider.dispatchEvent(new Event("input"));
    slider.dispatchEvent(new Event("change"));
  },
  async openRadar() {
    radarOpen = true;
    $("bt-radar-panel").hidden = false;
    setText("bt-radar-toggle", t().btRadarToggleClose);
    const snap = await invoke("bt_start_scan");
    renderRadar(snap);
    return snap;
  },
  async closeRadar() {
    radarOpen = false;
    $("bt-radar-panel").hidden = true;
    setText("bt-radar-toggle", t().btRadarToggleOpen);
    return invoke("bt_stop_scan");
  },
  report(kind) {
    return sendReport(kind);
  },
};

window.addEventListener("beforeunload", () => {
  if (radarOpen) invoke("bt_stop_scan").catch(() => {});
});

window.addEventListener("DOMContentLoaded", async () => {
  applyStaticStrings(initialLang());
  bind();
  // 저장 요청이 오가는 동안 오는 1초 알림은 바뀌기 전 값일 수 있으므로 건너뛴다.
  await listen("state", (e) => {
    if (inflight === 0) render(e.payload);
  });
  await listen("bt-radar", (e) => {
    if (radarOpen) renderRadar(e.payload);
  });
  render(await invoke("get_state"));
});
