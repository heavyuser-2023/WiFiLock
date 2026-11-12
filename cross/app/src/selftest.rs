//! 자가 진단 (`--self-test <폴더>`)
//!
//! 실제 앱을 띄운 채 시스템 기능·메뉴 아이콘·메뉴 동작·잠금 판단·경고 창·설정 창·설정 저장을 점검하고
//! 결과를 report.json·summary.txt 로 남긴 뒤 끝낸다. 화면은 절대 잠그지 않는다(시험 모드).
//!
//! - [통과]/[실패]: 반드시 맞아야 하는 항목 (하나라도 실패하면 종료 코드 1)
//! - [정보]: 환경에 따라 달라 값을 기록만 하는 항목 (예: 와이파이 장치가 없는 빌드 서버)

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use wifilock_core::{EngineState, SettingsStore, WiFiSnapshot, WiFiStatus};

use crate::engine;
use crate::menu;
use crate::state::{lock, state, FakeInputs};
use crate::ui::{self, IconKind};
use crate::views;

#[derive(Serialize)]
struct Check {
    name: String,
    ok: bool,
    /// "strict" = 반드시 통과, "info" = 기록만
    kind: &'static str,
    detail: String,
}

#[derive(Serialize)]
struct Report {
    os: &'static str,
    version: String,
    passed: usize,
    failed: usize,
    info: usize,
    checks: Vec<Check>,
}

struct SelfTest {
    app: AppHandle,
    dir: PathBuf,
    checks: Vec<Check>,
}

pub fn start(app: AppHandle, dir: PathBuf) {
    std::thread::Builder::new()
        .name("wifilock-selftest".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let mut t = SelfTest { app: app.clone(), dir, checks: Vec::new() };
            let code = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| t.run())) {
                Ok(()) => {
                    if t.checks.iter().any(|c| c.kind == "strict" && !c.ok) {
                        1
                    } else {
                        0
                    }
                }
                Err(e) => {
                    let msg = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    t.check("자가 진단 도중 멈춤 없음", false, msg);
                    2
                }
            };
            t.write_report();
            crate::logging::info(format!("자가 진단 끝: 종료 코드 {code}"));
            app.exit(code);
        })
        .expect("자가 진단 스레드");
}

/// 주 스레드에서 실행하고 결과를 기다린다.
fn main_sync<R: Send + 'static>(app: &AppHandle, f: impl FnOnce() -> R + Send + 'static) -> Option<R> {
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .ok()?;
    rx.recv_timeout(Duration::from_secs(15)).ok()
}

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    f()
}

impl SelfTest {
    fn check(&mut self, name: &str, ok: bool, detail: impl Into<String>) {
        let detail = detail.into();
        println!("[{}] {name}{}", if ok { "통과" } else { "실패" }, if detail.is_empty() { String::new() } else { format!(" — {detail}") });
        self.checks.push(Check { name: name.into(), ok, kind: "strict", detail });
    }

    fn info(&mut self, name: &str, detail: impl Into<String>) {
        let detail = detail.into();
        println!("[정보] {name} — {detail}");
        self.checks.push(Check { name: name.into(), ok: true, kind: "info", detail });
    }

    fn write_report(&self) {
        let passed = self.checks.iter().filter(|c| c.kind == "strict" && c.ok).count();
        let failed = self.checks.iter().filter(|c| c.kind == "strict" && !c.ok).count();
        let info = self.checks.iter().filter(|c| c.kind == "info").count();
        let report = Report {
            os: std::env::consts::OS,
            version: self.app.package_info().version.to_string(),
            passed,
            failed,
            info,
            checks: self.checks.iter().map(|c| Check { name: c.name.clone(), ok: c.ok, kind: c.kind, detail: c.detail.clone() }).collect(),
        };
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(j) = serde_json::to_string_pretty(&report) {
            let _ = std::fs::write(self.dir.join("report.json"), j);
        }
        let mut text = format!("자가 진단 ({}, 버전 {}): 통과 {passed} / 실패 {failed} / 정보 {info}\n\n", report.os, report.version);
        for c in &self.checks {
            let tag = match (c.kind, c.ok) {
                ("info", _) => "정보",
                (_, true) => "통과",
                _ => "실패",
            };
            text.push_str(&format!("[{tag}] {}{}\n", c.name, if c.detail.is_empty() { String::new() } else { format!(" — {}", c.detail) }));
        }
        let _ = std::fs::write(self.dir.join("summary.txt"), &text);
        println!("\n자가 진단: 통과 {passed} / 실패 {failed} / 정보 {info}");
    }

    fn run(&mut self) {
        self.check_system();
        self.check_tray_and_menu();
        self.check_menu_actions();
        self.check_real_display_hold();
        self.check_engine_and_hud();
        self.check_settings_window();
        self.check_persistence();
        let auto = crate::autostart_enabled(&self.app);
        self.info("로그인할 때 자동 실행 상태 읽기", if auto { "켜짐" } else { "꺼짐 (바꾸지 않음)" });
    }

    // ---------- 1. 시스템 기능 ----------

    fn check_system(&mut self) {
        let st = state(&self.app);
        let p = st.platform.clone();
        self.info("운영체제", format!("{} / {}", p.os(), std::env::consts::ARCH));
        match p.idle_seconds() {
            Some(v) => self.check("자리비움 시간 읽기", v.is_finite() && v >= 0.0, format!("{v:.1}초")),
            None if p.os() == "linux" => self.info("자리비움 시간 읽기", "이 데스크톱 환경에서는 읽을 수 없음 (잠금 판단을 하지 않고 상태로 알림)"),
            // 빌드 서버(사용자가 로그인한 화면이 없는 환경)에서는 입력 기록이 없을 수 있다
            None if std::env::var_os("CI").is_some() => self.info("자리비움 시간 읽기", "빌드 서버 환경이라 읽을 수 없음"),
            None => self.check("자리비움 시간 읽기", false, "읽을 수 없음"),
        }
        self.info("화면 잠김 상태 읽기", if p.screen_locked() { "지금 잠겨 있음" } else { "지금 잠겨 있지 않음" });
        let holders = p.display_sleep_preventers();
        self.info("화면 유지 요청 목록 읽기", if holders.is_empty() { "없음".into() } else { holders.join(", ") });
        let w = p.wifi_snapshot();
        self.info(
            "와이파이 상태 읽기",
            format!("{:?} / {} / 공유기 {}", w.status, w.korean_description(), w.router_mac.as_deref().unwrap_or("-")),
        );
        self.info("위치 권한 상태", format!("{:?}", p.location_status()));
        for (k, v) in p.diagnostics() {
            self.info(&format!("사용하는 방법: {k}"), v);
        }
        #[cfg(target_os = "macos")]
        self.check_macos_signature();
        let images_ok = [IconKind::Active, IconKind::Inactive, IconKind::Warning]
            .iter()
            .all(|k| ui::tray_image(*k).width() > 0);
        self.check("메뉴 아이콘 그림 3종", images_ok, "");
    }

    /// 맥: 강화된 실행 환경으로 서명된 앱은 위치 접근 엔타이틀먼트가 있어야 위치 권한 요청 창이 뜬다.
    /// (없으면 '위치 권한 요청' 단추를 눌러도 아무 반응이 없다)
    #[cfg(target_os = "macos")]
    fn check_macos_signature(&mut self) {
        let exe = std::env::current_exe().unwrap_or_default();
        let Some(bundle) = exe.ancestors().find(|p| p.extension().map(|e| e == "app").unwrap_or(false)) else {
            self.info("앱 서명 확인", "앱 번들 밖에서 실행 중이라 건너뜀");
            return;
        };
        let run = |args: &[&str]| {
            std::process::Command::new("/usr/bin/codesign")
                .args(args)
                .arg(bundle)
                .output()
                .map(|o| format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)))
                .unwrap_or_default()
        };
        let info = run(&["-dvv"]);
        let hardened = info.lines().any(|l| l.contains("flags=") && l.contains("runtime"));
        let ents = run(&["-d", "--entitlements", "-", "--xml"]);
        let has_location = ents.contains("com.apple.security.personal-information.location");
        if hardened {
            self.check(
                "강화된 실행 환경에서 위치 권한 요청 가능 (엔타이틀먼트)",
                has_location,
                if has_location { "com.apple.security.personal-information.location 있음" } else { "엔타이틀먼트 없음: 위치 권한 요청 창이 뜨지 않는다" },
            );
        } else {
            self.info("앱 서명 확인", "강화된 실행 환경이 아님");
        }
    }

    // ---------- 2. 메뉴 아이콘·메뉴 ----------

    fn menu_texts(&self) -> (String, String, String, bool, Vec<String>) {
        let st = state(&self.app);
        // 메뉴 핸들을 복사해 잠금을 놓은 뒤 글자를 읽는다 (글자 읽기는 주 스레드를 기다리므로)
        let h = lock(&st.menu).as_ref().map(|h| (h.network.clone(), h.policy.clone(), h.state.clone(), h.enabled.clone(), h.ids.clone()));
        let Some((n, p, s, e, ids)) = h else { return Default::default() };
        (n.text().unwrap_or_default(), p.text().unwrap_or_default(), s.text().unwrap_or_default(), e.is_checked().unwrap_or(false), ids)
    }

    fn check_tray_and_menu(&mut self) {
        let app = self.app.clone();
        let tray = main_sync(&app, {
            let a = app.clone();
            move || a.tray_by_id(ui::TRAY_ID).is_some()
        })
        .unwrap_or(false);
        self.check("메뉴 막대·알림 영역 아이콘 만들기", tray, "");
        let rect = main_sync(&app, {
            let a = app.clone();
            move || {
                let r = a.tray_by_id(ui::TRAY_ID).and_then(|t| t.rect().ok().flatten())?;
                let m = a.primary_monitor().ok().flatten()?;
                let pos = r.position.to_physical::<f64>(m.scale_factor());
                let size = r.size.to_physical::<f64>(m.scale_factor());
                Some((pos.x, pos.y, size.width, size.height, m.size().width as f64, m.size().height as f64))
            }
        })
        .flatten();
        match (std::env::consts::OS, rect) {
            ("macos", Some((x, y, w, h, sw, _sh))) => self.check(
                "아이콘 위치: 화면 오른쪽 위 메뉴 막대",
                y < 120.0 && x + w / 2.0 > sw / 2.0,
                format!("x {x:.0}, y {y:.0}, 크기 {w:.0}×{h:.0}, 화면 너비 {sw:.0}"),
            ),
            (_, Some((x, y, w, h, sw, sh))) => {
                self.info("아이콘 위치", format!("x {x:.0}, y {y:.0}, 크기 {w:.0}×{h:.0}, 화면 {sw:.0}×{sh:.0}"))
            }
            (_, None) => self.info("아이콘 위치", "이 환경에서는 위치를 알려 주지 않음"),
        }
        let (network, policy, _state, enabled, ids) = self.menu_texts();
        for (id, label) in [
            ("lock-now", "지금 잠그기"),
            ("enabled", "자동 잠금 사용"),
            ("pause", "일시 중지"),
            ("settings", "설정…"),
            ("about", "와이파이 잠금 정보"),
            ("quit", "종료"),
        ] {
            self.check(&format!("메뉴 항목 '{label}'"), ids.iter().any(|i| i == id), "");
        }
        self.check("메뉴에 네트워크 표시", network.starts_with("와이파이:"), network.clone());
        self.check("메뉴에 잠금 시간 표시", policy.starts_with("잠금 시간:"), policy.clone());
        self.check("'자동 잠금 사용' 체크 표시", enabled, "");
    }

    // ---------- 3. 메뉴 동작 (가짜 입력으로) ----------

    fn tick(&self) -> wifilock_core::TickResult {
        let r = engine::tick(&self.app);
        let a = self.app.clone();
        main_sync(&self.app, move || ui::refresh_now(&a));
        r
    }

    fn set_fake(&self, f: impl FnOnce(&mut FakeInputs)) {
        let st = state(&self.app);
        let mut g = lock(&st.fake);
        let fake = g.get_or_insert_with(|| FakeInputs {
            now: wifilock_core::now(),
            idle: 0.0,
            locked: false,
            holders: vec![],
            wifi: lock(&st.wifi).clone(),
        });
        f(fake);
    }

    fn menu_action(&self, id: &'static str) {
        let a = self.app.clone();
        main_sync(&self.app, move || menu::handle(&a, id));
    }

    fn check_menu_actions(&mut self) {
        let st = state(&self.app);
        st.manual.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(1100));
        self.set_fake(|f| {
            f.idle = 0.0;
            f.wifi = WiFiSnapshot::status(WiFiStatus::Disconnected);
        });
        self.tick();

        let before = st.lock_count.load(Ordering::SeqCst);
        self.menu_action("lock-now");
        let after = st.lock_count.load(Ordering::SeqCst);
        self.check("'지금 잠그기' 동작", after == before + 1, format!("잠금 요청 {before} → {after}"));

        self.menu_action("enabled");
        self.tick();
        let (_, _, s, checked, _) = self.menu_texts();
        let disabled = lock(&st.controller).status().state == EngineState::Disabled;
        self.check("자동 잠금 끄기", disabled && s == "자동 잠금이 꺼져 있음" && !checked, s);
        self.menu_action("enabled");
        self.tick();
        let on = lock(&st.controller).status().state != EngineState::Disabled;
        self.check("자동 잠금 다시 켜기", on, "");

        self.menu_action("pause-1800");
        self.tick();
        let (_, _, s, _, ids) = self.menu_texts();
        let paused = lock(&st.controller).is_paused();
        self.check(
            "일시 중지 30분",
            paused && s.starts_with("일시 중지됨") && ids.iter().any(|i| i == "resume") && !ids.iter().any(|i| i == "pause"),
            s,
        );
        self.menu_action("resume");
        self.tick();
        let (_, _, _, _, ids) = self.menu_texts();
        self.check("다시 시작", !lock(&st.controller).is_paused() && ids.iter().any(|i| i == "pause"), "");
    }

    // ---------- 4. 실제 화면 유지 요청 감지 ----------

    fn check_real_display_hold(&mut self) {
        let st = state(&self.app);
        let p = st.platform.clone();
        match p.os() {
            "macos" => {
                let child = std::process::Command::new("/usr/bin/caffeinate").args(["-d", "-t", "30"]).spawn();
                let Ok(mut child) = child else {
                    self.check("실제 화면 유지 요청 감지 (caffeinate -d)", false, "caffeinate 실행 실패");
                    return;
                };
                let seen = wait_until(Duration::from_secs(5), || p.display_sleep_preventers().iter().any(|n| n == "caffeinate"));
                self.check("실제 화면 유지 요청 감지 (caffeinate -d)", seen, p.display_sleep_preventers().join(", "));
                let _ = child.kill();
                let _ = child.wait();
                let gone = wait_until(Duration::from_secs(5), || !p.display_sleep_preventers().iter().any(|n| n == "caffeinate"));
                self.check("요청이 끝나면 보류 해제", gone, "");
            }
            "windows" => {
                let before = p.display_sleep_preventers();
                wifilock_platform::test_display_hold(true);
                let seen = p.display_sleep_preventers();
                wifilock_platform::test_display_hold(false);
                let after = p.display_sleep_preventers();
                self.info(
                    "화면 유지 요청 감지 (SetThreadExecutionState)",
                    format!("요청 전 {:?} → 요청 중 {:?} → 해제 후 {:?}", before, seen, after),
                );
            }
            _ => {
                self.info("화면 유지 요청 감지", format!("지금 목록: {:?}", p.display_sleep_preventers()));
            }
        }
    }

    // ---------- 5. 잠금 판단 + 경고 창 (가짜 시간·와이파이) ----------

    fn advance(&self, seconds: usize) -> Vec<wifilock_core::TickResult> {
        (0..seconds)
            .map(|_| {
                self.set_fake(|f| {
                    f.now += 1.0;
                    f.idle += 1.0;
                });
                self.tick()
            })
            .collect()
    }

    fn check_engine_and_hud(&mut self) {
        let st = state(&self.app);
        let home = WiFiSnapshot::new(WiFiStatus::Connected, Some("SelfTestHome"), Some("02:00:00:00:00:01"), Some("wlan-test"));
        let cafe = WiFiSnapshot::connected(Some("SelfTestCafe"), Some("02:00:00:00:00:99"));
        {
            let mut s = lock(&st.store);
            s.update(|x| {
                x.enabled = true;
                x.unknown_wifi_timeout_seconds = 120;
                x.no_wifi_timeout_seconds = 60;
                x.warn_before_lock = true;
                x.warning_seconds = 10;
                x.respect_display_sleep_assertions = true;
            });
            s.add_rule_for(&home, Some(1800));
        }
        self.set_fake(|f| {
            f.wifi = home.clone();
            f.idle = 0.0;
            f.locked = false;
            f.holders.clear();
        });
        self.tick();
        let (_, policy, _, _, _) = self.menu_texts();
        self.check("시나리오: 등록한 집 와이파이 → 30분", policy.contains("30분") && policy.contains("SelfTestHome"), policy);

        // 낯선 와이파이로 바뀜 → 15초 확인 뒤 2분 규칙
        self.set_fake(|f| {
            f.wifi = cafe.clone();
            f.idle = 0.0;
        });
        let outs = self.advance(20);
        let (_, policy, _, _, _) = self.menu_texts();
        let pending_seen = outs.iter().take(10).all(|r| r.lock_reason.is_none());
        self.check("시나리오: 낯선 와이파이 15초 확인 뒤 2분 규칙", policy.contains("2분") && pending_seen, policy);

        // 경고 시작까지
        let outs = self.advance(95);
        let warn_at = outs.iter().position(|r| r.output.warning_remaining().is_some()).map(|i| i + 21);
        let hud_shown = views::hud_visible(&self.app) && lock(&st.hud_remaining).is_some();
        let (_, _, s, _, _) = self.menu_texts();
        self.check("시나리오: 잠금 10초 전 경고", warn_at == Some(110), format!("경고 시작 {}초", warn_at.map(|v| v.to_string()).unwrap_or("-".into())));
        self.check("잠금 직전 경고 창 표시", hud_shown, s.clone());
        self.check("메뉴에 '곧 잠김' 표시", s.starts_with("곧 잠김"), s.clone());
        let hud_val = main_sync(&self.app, {
            let a = self.app.clone();
            move || crate::commands::hud_state(a)
        })
        .flatten();
        self.check("경고 창 남은 시간", hud_val.map(|v| (1..=10).contains(&v)).unwrap_or(false), format!("{hud_val:?}"));

        // 입력이 생기면 경고 취소
        self.set_fake(|f| f.idle = 0.2);
        self.tick();
        let hidden = wait_until(Duration::from_secs(3), || !views::hud_visible(&self.app));
        self.check("입력이 생기면 경고 창 사라짐", hidden && lock(&st.hud_remaining).is_none(), "");

        // 끝까지 자리비움 → 잠금(시험 모드에서는 기록만)
        let before = st.lock_count.load(Ordering::SeqCst);
        let outs = self.advance(125);
        let lock_at = outs.iter().position(|r| r.lock_reason.is_some()).map(|i| i + 1);
        let after = st.lock_count.load(Ordering::SeqCst);
        self.check(
            "시나리오: 2분 자리비움 → 잠금",
            after == before + 1 && lock_at == Some(120),
            format!("잠금 시점 {}초, 요청 {before} → {after}", lock_at.map(|v| v.to_string()).unwrap_or("-".into())),
        );
        let hidden = wait_until(Duration::from_secs(3), || !views::hud_visible(&self.app));
        self.check("잠근 뒤 경고 창 닫힘", hidden, "");

        // 화면이 잠겨 있으면 표시
        self.set_fake(|f| f.locked = true);
        self.tick();
        let (_, _, s, _, _) = self.menu_texts();
        self.check("화면 잠김 상태 표시", s == "화면 잠김", s);

        // 풀리면(입력 없이) 그 시각부터 다시 센다
        self.set_fake(|f| f.locked = false);
        let outs = self.advance(30);
        self.check("잠금 해제 직후 바로 다시 잠그지 않음", outs.iter().all(|r| r.lock_reason.is_none() && r.output.warning_remaining().is_none()), "");

        // 화면 유지 요청이 있으면 보류
        self.set_fake(|f| {
            f.idle = 0.0;
            f.holders = vec!["동영상 재생기".into()];
        });
        self.advance(200);
        let (_, _, s, _, _) = self.menu_texts();
        self.check("동영상 재생 중 보류 표시", s.contains("잠금 보류") && s.contains("동영상 재생기"), s);
        self.set_fake(|f| {
            f.holders.clear();
            f.idle = 0.0;
        });
        self.tick();
    }

    // ---------- 6. 설정 창 ----------

    fn eval_settings(&self, js: &'static str) -> bool {
        let a = self.app.clone();
        main_sync(&self.app, move || a.get_webview_window(views::SETTINGS).map(|w| w.eval(js).is_ok()).unwrap_or(false))
            .unwrap_or(false)
    }

    fn check_settings_window(&mut self) {
        let st = state(&self.app);
        *lock(&st.page_report) = None;
        let a = self.app.clone();
        main_sync(&self.app, move || views::open_settings(&a));
        let loaded = wait_until(Duration::from_secs(20), || lock(&st.page_report).is_some());
        let report = lock(&st.page_report).clone().unwrap_or_default();
        self.check("설정 창 열기", loaded, report.get("title").and_then(|v| v.as_str()).unwrap_or("응답 없음").to_string());
        if !loaded {
            return;
        }
        self.check("설정 창 제목", report.get("title").and_then(|v| v.as_str()) == Some("와이파이 잠금 설정"), "");
        let sections: Vec<String> = report
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();
        for name in ["일반", "현재 네트워크", "네트워크별 잠금 시간", "기본 잠금 시간", "고급"] {
            self.check(&format!("설정 창 항목 '{name}'"), sections.iter().any(|s| s == name), "");
        }
        let rules = report.get("ruleCount").and_then(|v| v.as_u64()).unwrap_or(0);
        self.check("설정 창에 등록된 네트워크 표시", rules >= 1, format!("{rules}개"));

        // 화면에서 값을 바꾸면 저장되고 메뉴에 반영된다
        self.eval_settings("window.wifilockTest && window.wifilockTest.setUnknown(600)");
        let saved = wait_until(Duration::from_secs(10), || lock(&st.store).settings().unknown_wifi_timeout_seconds == 600);
        self.check("설정 창에서 바꾼 값 저장", saved, "");
        self.set_fake(|f| f.wifi = WiFiSnapshot::connected(Some("SelfTestCafe"), Some("02:00:00:00:00:99")));
        self.advance(1);
        let (_, policy, _, _, _) = self.menu_texts();
        self.check("설정 변경이 메뉴에 반영", policy.contains("10분"), policy);

        self.eval_settings("window.wifilockTest && window.wifilockTest.addByName('SelfTestOffice', 900)");
        let added = wait_until(Duration::from_secs(10), || {
            lock(&st.store).settings().rules.iter().any(|r| r.ssid.as_deref() == Some("SelfTestOffice") && r.timeout_seconds == 900)
        });
        self.check("설정 창에서 이름으로 네트워크 추가", added, "");

        // 삭제 단추에 초점이 있는 채로도 목록이 바로 다시 그려져야 한다 (윈도우·리눅스 웹뷰는 누른 단추로 초점이 옮겨 감)
        self.eval_settings("window.wifilockTest && window.wifilockTest.deleteByName('SelfTestOffice')");
        let removed = wait_until(Duration::from_secs(10), || {
            !lock(&st.store).settings().rules.iter().any(|r| r.ssid.as_deref() == Some("SelfTestOffice"))
        });
        self.check("설정 창에서 네트워크 삭제", removed, "");
        let expected = lock(&st.store).settings().rules.len() as u64;
        let mut shown = None;
        let redrawn = wait_until(Duration::from_secs(10), || {
            *lock(&st.page_report) = None;
            self.eval_settings("window.wifilockTest && window.wifilockTest.report('after-delete')");
            let got = wait_until(Duration::from_secs(2), || lock(&st.page_report).is_some());
            let r = lock(&st.page_report).clone().unwrap_or_default();
            shown = r.get("ruleCount").and_then(|v| v.as_u64());
            got && shown == Some(expected)
        });
        self.check("삭제 뒤 설정 창 목록 갱신", redrawn, format!("화면 {shown:?}개 / 저장 {expected}개"));

        let a = self.app.clone();
        main_sync(&self.app, move || {
            if let Some(w) = a.get_webview_window(views::SETTINGS) {
                let _ = w.close();
            }
        });
        let closed = wait_until(Duration::from_secs(10), || {
            let a = self.app.clone();
            main_sync(&self.app, move || a.get_webview_window(views::SETTINGS).is_none()).unwrap_or(false)
        });
        self.check("설정 창 닫기 (앱은 계속 실행)", closed, "");
    }

    // ---------- 7. 설정 저장 ----------

    fn check_persistence(&mut self) {
        let st = state(&self.app);
        let current = lock(&st.store).settings().clone();
        let path = lock(&st.store).path().map(|p| p.to_path_buf());
        let Some(path) = path else {
            self.check("설정 저장·불러오기", false, "저장 위치 없음");
            return;
        };
        let reopened = SettingsStore::open(&path).settings().clone();
        self.check("설정 저장·불러오기", reopened == current && !reopened.rules.is_empty(), format!("{}", path.display()));
        let _ = self.app.state::<std::sync::Arc<crate::state::AppState>>();
    }
}
