"use strict";
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

function update(seconds) {
  if (seconds == null) return;
  document.getElementById("title").textContent = `${Math.max(1, seconds)}초 후 화면이 잠깁니다`;
}

window.addEventListener("DOMContentLoaded", async () => {
  await listen("hud", (e) => update(e.payload));
  update(await invoke("hud_state"));
});
