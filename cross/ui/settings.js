// 와이파이 잠금 설정 창
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let state = null;
let lastRulesKey = "";
let reported = false;

const $ = (id) => document.getElementById(id);

const ICONS = {
  on: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0"/><path d="M5 12.6a10 10 0 0 1 14 0"/><path d="M8.5 16.3a5 5 0 0 1 7 0"/><circle cx="12" cy="19.6" r="1.1" fill="currentColor"/></svg>',
  off: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M8.5 16.3a5 5 0 0 1 7 0" opacity=".35"/><path d="M3 3l18 18"/></svg>',
  warn: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M2 8.8a15 15 0 0 1 20 0" opacity=".35"/><path d="M5 12.6a10 10 0 0 1 14 0" opacity=".35"/><path d="M12 9v6"/><circle cx="12" cy="19.2" r="1.1" fill="currentColor"/></svg>',
};

function labelOf(value) {
  const found = state.timeoutLabels.find((c) => c.value === value);
  return found ? found.label : `${value}초`;
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
  const key = values.join(",") + "|" + current;
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
  if (document.activeElement !== e && e.checked !== value) e.checked = value;
}

async function call(cmd, args) {
  try {
    const next = await invoke(cmd, args);
    if (next) render(next);
    return next;
  } catch (e) {
    console.error(cmd, e);
    throw e;
  }
}

/** 설정 일부를 바꿔 저장한다. */
function save(mutate) {
  const next = JSON.parse(JSON.stringify(state.settings));
  mutate(next);
  return call("save_settings", { settings: next });
}

function renderRules() {
  const s = state.settings;
  const key = JSON.stringify([s.rules, state.currentRuleId, state.timeoutLabels]);
  const box = $("rules");
  // 이름을 고치는 중에는 다시 그리지 않는다 (입력이 끊기지 않도록)
  if (key === lastRulesKey || box.contains(document.activeElement)) return;
  lastRulesKey = key;

  if (s.rules.length === 0) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent = "등록된 네트워크가 없습니다. 집이나 사무실처럼 믿을 수 있는 네트워크를 등록하면 그곳에서는 잠금 시간을 길게 둘 수 있습니다.";
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
    mark.title = current ? "지금 연결된 네트워크" : "";

    const text = document.createElement("div");
    text.className = "text";
    const name = document.createElement("input");
    name.type = "text";
    name.className = "name";
    name.value = rule.name;
    name.setAttribute("aria-label", "네트워크 이름");
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
    if (rule.ssid) parts.push(`와이파이 이름: ${rule.ssid}`);
    if (rule.routerMAC) parts.push(`공유기: ${rule.routerMAC}`);
    ident.textContent = parts.join(" · ");
    text.append(name, ident);

    const select = document.createElement("select");
    select.setAttribute("aria-label", "잠금 시간");
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
    up.title = "위로 (먼저 적용)";
    up.disabled = index === 0;
    up.addEventListener("click", () => call("move_rule", { id: rule.id, up: true }));
    const down = document.createElement("button");
    down.className = "icon";
    down.textContent = "↓";
    down.title = "아래로";
    down.disabled = index === s.rules.length - 1;
    down.addEventListener("click", () => call("move_rule", { id: rule.id, up: false }));
    order.append(up, down);

    const del = document.createElement("button");
    del.className = "icon danger";
    del.textContent = "삭제";
    del.title = "이 네트워크 삭제";
    del.addEventListener("click", () => call("remove_rule", { id: rule.id }));

    row.append(mark, text, select, order, del);
    return row;
  });
  box.replaceChildren(...rows);
}

function render(next) {
  state = next;
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
  setText("state-text", state.stateText + (state.dryRun ? " (시험 모드: 실제로 잠그지 않음)" : ""));
  $("add-current-inline").hidden = !state.canAddCurrent;
  $("add-current").disabled = !state.canAddCurrent;

  const loc = state.location;
  $("permission").hidden = !loc.needsPermission;
  setText("perm-title", loc.title);
  setText("perm-expl", loc.explanation);
  setText("perm-button", loc.button);

  renderRules();

  // 기본 잠금 시간
  fillSelect($("unknown"), timeoutChoices(s.unknownWiFiTimeoutSeconds), s.unknownWiFiTimeoutSeconds, labelOf);
  fillSelect($("nowifi"), timeoutChoices(s.noWiFiTimeoutSeconds), s.noWiFiTimeoutSeconds, labelOf);
  if (!$("new-timeout").dataset.key) {
    fillSelect($("new-timeout"), timeoutChoices(1800), 1800, labelOf);
  }

  // 고급
  setChecked("warn", s.warnBeforeLock);
  fillSelect($("warning"), state.warningOptions.map((c) => c.value), s.warningSeconds, (v) => {
    const c = state.warningOptions.find((x) => x.value === v);
    return c ? c.label : `${v}초`;
  });
  $("warning").disabled = !s.warnBeforeLock;
  setChecked("respect", s.respectDisplaySleepAssertions);
  $("holders-note").hidden = !state.holdersNote;
  setText("holders-note", state.holdersNote);
  $("countdown-row").hidden = !state.countdownSupported;
  setChecked("countdown", s.showCountdownInMenuBar);

  const osName = { macos: "맥", windows: "윈도우", linux: "리눅스" }[state.os] || state.os;
  setText("about", `와이파이 잠금 ${state.version} · ${osName}`);

  if (!reported) {
    reported = true;
    invoke("page_report", {
      report: {
        title: document.title,
        sections: [...document.querySelectorAll("section > h2")].map((h) => h.textContent),
        ruleCount: state.settings.rules.length,
        os: state.os,
      },
    }).catch(() => {});
  }
}

function bind() {
  $("enabled").addEventListener("change", (e) => save((n) => (n.enabled = e.target.checked)));
  $("autostart").addEventListener("change", (e) => call("set_autostart", { enabled: e.target.checked }));
  $("warn").addEventListener("change", (e) => save((n) => (n.warnBeforeLock = e.target.checked)));
  $("respect").addEventListener("change", (e) => save((n) => (n.respectDisplaySleepAssertions = e.target.checked)));
  $("countdown").addEventListener("change", (e) => save((n) => (n.showCountdownInMenuBar = e.target.checked)));
  $("unknown").addEventListener("change", (e) => save((n) => (n.unknownWiFiTimeoutSeconds = Number(e.target.value))));
  $("nowifi").addEventListener("change", (e) => save((n) => (n.noWiFiTimeoutSeconds = Number(e.target.value))));
  $("warning").addEventListener("change", (e) => save((n) => (n.warningSeconds = Number(e.target.value))));

  $("add-current").addEventListener("click", () => call("add_current_network"));
  $("add-current-inline").addEventListener("click", () => call("add_current_network"));
  $("perm-button").addEventListener("click", () => invoke("request_location"));

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
      showError("와이파이 이름을 입력해 주세요.");
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
};

window.addEventListener("DOMContentLoaded", async () => {
  bind();
  await listen("state", (e) => render(e.payload));
  render(await invoke("get_state"));
});
