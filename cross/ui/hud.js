"use strict";
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function isKorean() {
  const q = new URLSearchParams(window.location.search).get("lang");
  if (q === "ko") return true;
  if (q === "en") return false;
  return (navigator.language || "").toLowerCase().startsWith("ko");
}

function applyStatic() {
  if (isKorean()) return;
  document.documentElement.lang = "en";
  document.title = "WiFiLock Warning";
  document.getElementById("title").textContent = "Screen will lock soon";
  const detail = document.querySelector(".detail");
  if (detail) detail.textContent = "Move the mouse or press any key to cancel";
}

function update(seconds) {
  if (seconds == null) return;
  const s = Math.max(1, seconds);
  document.getElementById("title").textContent = isKorean()
    ? `${s}초 후 화면이 잠깁니다`
    : `Locking screen in ${s}s`;
}

window.addEventListener("DOMContentLoaded", async () => {
  applyStatic();
  await listen("hud", (e) => update(e.payload));
  update(await invoke("hud_state"));
});
