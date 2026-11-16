// 와이파이 잠금 설정 창
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let state = null;
let lastRulesKey = "";
let appliedLang = "";
let reported = false;
/** 보낸 요청 번호와 아직 응답이 오지 않은 요청 수 (늦게 온 응답·알림이 새 값을 덮어쓰지 않게) */
let requestSeq = 0;
let inflight = 0;

const $ = (id) => document.getElementById(id);

const ICONS = {
  on: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0"/><path d="M5 12.6a10 10 0 0 1 14 0"/><path d="M8.5 16.3a5 5 0 0 1 7 0"/><circle cx="12" cy="19.6" r="1.1" fill="currentColor"/></svg>',
  off: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M8.5 16.3a5 5 0 0 1 7 0" opacity=".35"/><path d="M3 3l18 18"/></svg>',
  warn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M12 9v6"/><circle cx="12" cy="19.2" r="1.1" fill="currentColor"/></svg>',
};

const STRINGS = {
  ko: {
    title: "와이파이 잠금 설정",
    secGeneral: "일반",
    enabledLabel: "자동 잠금 사용",
    enabledDesc: "키보드·마우스 입력이 없으면 아래 시간 뒤에 화면을 잠급니다.",
    autostartLabel: "로그인할 때 자동으로 실행",
    secCurrent: "현재 네트워크",
    checking: "확인 중…",
    addCurrentInline: "등록",
    secRules: "네트워크별 잠금 시간",
    addCurrent: "＋ 현재 네트워크 추가",
    addByName: "이름으로 추가…",
    addFormLabel: "와이파이 이름으로 추가",
    addFormDesc: "대소문자까지 정확히 입력해 주세요.",
    newSsidPlaceholder: "와이파이 이름",
    addCancel: "취소",
    addCommit: "추가",
    rulesFooter:
      "위에 있는 규칙부터 적용됩니다. 공유기 주소까지 기록된 규칙은 이름이 같아도 공유기가 다르면 적용하지 않습니다. 이름으로만 추가한 규칙은 같은 이름의 모든 와이파이에 적용됩니다.",
    secDefaults: "기본 잠금 시간",
    unknownLabel: "등록되지 않은 와이파이",
    nowifiLabel: "와이파이 꺼짐 · 연결 안 됨",
    defaultsFooter: "등록되지 않은 곳에서는 짧게 두면 노트북을 두고 자리를 비웠을 때 더 안전합니다.",
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
    secCurrent: "Current Network",
    checking: "Checking…",
    addCurrentInline: "Add",
    secRules: "Per-Network Lock Times",
    addCurrent: "＋ Add Current Network",
    addByName: "Add by Name…",
    addFormLabel: "Add by Wi-Fi Name",
    addFormDesc: "Enter the exact case-sensitive Wi-Fi name.",
    newSsidPlaceholder: "Wi-Fi name",
    addCancel: "Cancel",
    addCommit: "Add",
    rulesFooter:
      "Rules are applied from top to bottom. Rules with a router MAC address only apply to that specific router, even if another Wi-Fi has the same name. Rules added by name only apply to any Wi-Fi with that name.",
    secDefaults: "Default Lock Times",
    unknownLabel: "Unregistered Wi-Fi",
    nowifiLabel: "Wi-Fi Off · Not Connected",
    defaultsFooter: "Using a shorter timeout for unregistered networks helps keep your computer secure away from home.",
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
  setSel("#sec-general .row:nth-child(1) .label", s.enabledLabel);
  setSel("#sec-general .row:nth-child(1) .desc", s.enabledDesc);
  setSel("#sec-general .row:nth-child(2) .label", s.autostartLabel);

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
  setSel("#sec-rules > p.footer", s.rulesFooter);

  setSel("#sec-defaults > h2", s.secDefaults);
  setSel("#sec-defaults .row:nth-child(1) .label", s.unknownLabel);
  setSel("#sec-defaults .row:nth-child(2) .label", s.nowifiLabel);
  setSel("#sec-defaults > p.footer", s.defaultsFooter);

  setSel("#sec-advanced > h2", s.secAdvanced);
  setSel("#sec-advanced .row:nth-child(1) .label", s.warnLabel);
  setSel("#sec-advanced .row:nth-child(2) .label", s.warningLabel);
  setSel("#sec-advanced .row:nth-child(2) .desc", s.warningDesc);
  setSel("#sec-advanced .row:nth-child(3) .label", s.respectLabel);
  setSel("#sec-advanced .row:nth-child(3) .desc", s.respectDesc);
  setSel("#countdown-row .label", s.countdownLabel);
  setSel("#countdown-row .desc", s.countdownDesc);
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

  // 기본 잠금 시간
  fillSelect($("unknown"), timeoutChoices(s.unknownWiFiTimeoutSeconds), s.unknownWiFiTimeoutSeconds, labelOf);
  fillSelect($("nowifi"), timeoutChoices(s.noWiFiTimeoutSeconds), s.noWiFiTimeoutSeconds, labelOf);
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
  $("unknown").addEventListener("change", (e) => save((n) => (n.unknownWiFiTimeoutSeconds = Number(e.target.value))));
  $("nowifi").addEventListener("change", (e) => save((n) => (n.noWiFiTimeoutSeconds = Number(e.target.value))));
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
  report(kind) {
    return sendReport(kind);
  },
};

window.addEventListener("DOMContentLoaded", async () => {
  applyStaticStrings(initialLang());
  bind();
  // 저장 요청이 오가는 동안 오는 1초 알림은 바뀌기 전 값일 수 있으므로 건너뛴다.
  await listen("state", (e) => {
    if (inflight === 0) render(e.payload);
  });
  render(await invoke("get_state"));
});
