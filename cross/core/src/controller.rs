use serde::Serialize;

use crate::format;
use crate::policy::{LockPolicy, PolicyResolver, PolicySource, WiFiSnapshot};
use crate::scheduler::{EngineState, LockScheduler, TickInput, TickOutput};
use crate::settings::AppSettings;
use crate::stabilizer::PolicyStabilizer;
use crate::Timestamp;

/// 일시 중지: None = 중지 아님, Some(None) = 다시 시작할 때까지, Some(Some(t)) = t 까지
pub type PauseState = Option<Option<Timestamp>>;

/// 시간 간격이 이보다 크면 컴퓨터가 잠자기에서 깨어난 것으로 본다.
const WAKE_GAP_SECONDS: f64 = 30.0;

/// 화면에 보여 줄 현재 상황
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockStatus {
    pub state: EngineState,
    pub policy: LockPolicy,
    /// 실제 와이파이 상태 (메뉴 표시용)
    pub snapshot: WiFiSnapshot,
    /// 와이파이 변화가 잠금 시간을 줄이는 쪽이라 확인 중 (순간 끊김 대비)
    pub policy_change_pending: bool,
    pub last_lock_at: Option<Timestamp>,
    pub paused: bool,
}

impl Default for LockStatus {
    fn default() -> Self {
        LockStatus {
            state: EngineState::Counting { remaining: 0.0, timeout: 0.0 },
            policy: LockPolicy::new(0, PolicySource::NoWiFi),
            snapshot: WiFiSnapshot::unknown(),
            policy_change_pending: false,
            last_lock_at: None,
            paused: false,
        }
    }
}

/// 매 초 운영체제에서 읽어 넘겨주는 값
#[derive(Debug, Clone)]
pub struct SystemInputs {
    pub now: Timestamp,
    /// 마지막 입력 후 지난 초. 이 컴퓨터에서 읽을 수 없으면 None.
    pub idle_seconds: Option<f64>,
    /// 화면이 잠겨 있거나 이 사용자 세션이 화면 앞에 없음
    pub screen_locked: bool,
    /// 화면 유지를 요청한 앱 이름들
    pub assertion_holders: Vec<String>,
    pub wifi: WiFiSnapshot,
}

#[derive(Debug, Clone)]
pub struct TickResult {
    pub output: TickOutput,
    /// Some 이면 지금 화면을 잠가야 한다 (잠근 이유)
    pub lock_reason: Option<String>,
    /// 화면에 보이는 상태가 달라졌는지
    pub changed: bool,
}

/// 매 초 시스템 상태를 받아 잠금 여부를 판단한다. 실제 잠금은 호출한 쪽이 한다.
pub struct Controller {
    scheduler: LockScheduler,
    stabilizer: PolicyStabilizer,
    pause: PauseState,
    /// 잠금 해제·잠자기에서 깨어난 시각. 입력 없이(애플워치·얼굴 인식 등) 풀린 경우에도 이 시각부터 자리비움을 센다.
    last_presence_at: Option<Timestamp>,
    status: LockStatus,
    override_timeout: Option<i64>,
    last_screen_locked: Option<bool>,
    last_tick_at: Option<Timestamp>,
}

impl Controller {
    pub fn new(override_timeout: Option<i64>) -> Self {
        Controller {
            scheduler: LockScheduler::new(),
            stabilizer: PolicyStabilizer::new(15.0),
            pause: None,
            last_presence_at: None,
            status: LockStatus::default(),
            override_timeout,
            last_screen_locked: None,
            last_tick_at: None,
        }
    }

    pub fn status(&self) -> &LockStatus {
        &self.status
    }

    pub fn pause_state(&self) -> PauseState {
        self.pause
    }

    pub fn is_paused(&self) -> bool {
        self.pause.is_some()
    }

    /// 사용자가 자리에 있다는 신호 (잠금 해제, 깨어남, 세션 복귀)
    pub fn note_presence(&mut self, now: Timestamp) {
        self.last_presence_at = Some(now);
        self.scheduler.note_user_activity();
    }

    pub fn pause_for(&mut self, now: Timestamp, seconds: Option<f64>) {
        self.pause = Some(seconds.map(|s| now + s));
        self.status.paused = true;
    }

    pub fn resume(&mut self) {
        self.pause = None;
        self.status.paused = false;
    }

    /// 잠금 명령을 보냈음을 기록한다.
    pub fn did_lock(&mut self, now: Timestamp) {
        self.status.last_lock_at = Some(now);
    }

    /// 잠금 시간을 정할 때 쓰는 와이파이 상태 (순간 끊김 대비를 거친 값)
    pub fn current_policy(&self, settings: &AppSettings, wifi: &WiFiSnapshot) -> LockPolicy {
        let snap = self.stabilizer.stable().unwrap_or(wifi);
        let mut p = PolicyResolver::resolve(snap, settings);
        if let Some(o) = self.override_timeout {
            p.timeout_seconds = o;
        }
        p
    }

    pub fn tick(&mut self, settings: &AppSettings, input: SystemInputs) -> TickResult {
        let now = input.now;

        // 잠금이 풀렸거나, 오래 멈춰 있다가(잠자기) 다시 돌기 시작했으면 사용자가 돌아온 것으로 본다.
        if self.last_screen_locked == Some(true) && !input.screen_locked {
            self.note_presence(now);
        }
        if let Some(prev) = self.last_tick_at {
            if now - prev > WAKE_GAP_SECONDS {
                self.note_presence(now);
            }
        }
        self.last_screen_locked = Some(input.screen_locked);
        self.last_tick_at = Some(now);

        if let Some(Some(until)) = self.pause {
            if until <= now {
                self.pause = None;
            }
        }

        self.stabilizer.update(&input.wifi, settings, now);
        let policy = self.current_policy(settings, &input.wifi);

        let mut output;
        let mut lock_reason = None;
        match input.idle_seconds {
            None => {
                output = TickOutput { state: EngineState::IdleUnavailable, should_lock: false };
                // 잠김·꺼짐·일시 중지 같은 상태는 그대로 보여 준다.
                let probe = TickInput {
                    now,
                    idle_seconds: 0.0,
                    policy: policy.clone(),
                    enabled: settings.enabled,
                    pause: self.pause,
                    screen_is_locked: input.screen_locked,
                    assertion_holders: Vec::new(),
                    warn_before_lock: false,
                    warning_seconds: 0.0,
                };
                let o = LockScheduler::new().tick(&probe);
                if !matches!(o.state, EngineState::Counting { .. }) {
                    output.state = o.state;
                }
            }
            Some(raw_idle) => {
                let mut idle = raw_idle.max(0.0);
                if let Some(p) = self.last_presence_at {
                    idle = idle.min((now - p).max(0.0));
                }
                let holders = if settings.respect_display_sleep_assertions { input.assertion_holders.clone() } else { Vec::new() };
                let tick_input = TickInput {
                    now,
                    idle_seconds: idle,
                    policy: policy.clone(),
                    enabled: settings.enabled,
                    pause: self.pause,
                    screen_is_locked: input.screen_locked,
                    assertion_holders: holders,
                    warn_before_lock: settings.warn_before_lock,
                    warning_seconds: settings.warning_seconds as f64,
                };
                output = self.scheduler.tick(&tick_input);
                if output.should_lock {
                    lock_reason = Some(format!(
                        "{}, 자리비움 {}초 ≥ {}",
                        policy.source.korean_description(),
                        idle as i64,
                        format::timeout(policy.timeout_seconds)
                    ));
                }
            }
        }

        let new_status = LockStatus {
            state: output.state.clone(),
            policy,
            snapshot: input.wifi.clone(),
            policy_change_pending: self.stabilizer.pending_since().is_some(),
            last_lock_at: self.status.last_lock_at,
            paused: self.pause.is_some(),
        };
        let changed = new_status != self.status;
        self.status = new_status;
        TickResult { output, lock_reason, changed }
    }
}
