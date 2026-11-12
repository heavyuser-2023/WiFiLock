use serde::Serialize;

use crate::policy::LockPolicy;
use crate::Timestamp;

/// 잠금 엔진이 매 초 판단한 상태
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EngineState {
    /// 사용자가 자동 잠금을 껐음
    Disabled,
    /// 일시 중지 중 (None 이면 다시 켤 때까지)
    Paused { until: Option<Timestamp> },
    /// 현재 네트워크 규칙이 "잠그지 않음"
    NeverLock,
    /// 다른 앱이 화면 유지를 요청해 보류 중
    HeldByAssertion { holders: Vec<String> },
    /// 화면이 이미 잠겨 있음
    ScreenLocked,
    /// 잠금까지 남은 시간을 세는 중
    Counting { remaining: f64, timeout: f64 },
    /// 잠금 직전 경고 중
    Warning { remaining: f64 },
    /// 방금 잠금 명령을 보냄
    LockTriggered,
    /// 이미 이번 자리비움 동안 잠갔으므로 사용자가 다시 움직일 때까지 기다림
    WaitingForActivity,
    /// 이 컴퓨터에서 자리비움 시간을 읽을 수 없음 (일부 리눅스 데스크톱)
    IdleUnavailable,
}

/// 한 번의 판단에 필요한 입력값
#[derive(Debug, Clone)]
pub struct TickInput {
    pub now: Timestamp,
    /// 마지막 키보드·마우스 입력 후 지난 시간(초)
    pub idle_seconds: f64,
    pub policy: LockPolicy,
    pub enabled: bool,
    /// 일시 중지: None = 중지 아님, Some(None) = 무기한, Some(Some(t)) = t 까지
    pub pause: Option<Option<Timestamp>>,
    pub screen_is_locked: bool,
    /// 화면 유지를 요청한 앱 이름 (설정에서 존중하지 않기로 했으면 빈 목록)
    pub assertion_holders: Vec<String>,
    pub warn_before_lock: bool,
    pub warning_seconds: f64,
}

impl TickInput {
    /// 시험·간단한 호출용 기본값 (켜짐, 경고 10초)
    pub fn basic(now: Timestamp, idle_seconds: f64, policy: LockPolicy) -> Self {
        TickInput {
            now,
            idle_seconds,
            policy,
            enabled: true,
            pause: None,
            screen_is_locked: false,
            assertion_holders: Vec::new(),
            warn_before_lock: true,
            warning_seconds: 10.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TickOutput {
    pub state: EngineState,
    /// true 면 지금 화면을 잠가야 한다
    pub should_lock: bool,
}

impl TickOutput {
    fn new(state: EngineState, should_lock: bool) -> Self {
        TickOutput { state, should_lock }
    }

    pub fn warning_remaining(&self) -> Option<f64> {
        match self.state {
            EngineState::Warning { remaining } => Some(remaining),
            _ => None,
        }
    }
}

/// 자리비움 시간과 현재 규칙으로 잠글 시점을 정하는 상태 기계.
///
/// - 같은 자리비움 동안에는 한 번만 잠근다. 사용자가 다시 입력하면(자리비움 시간이 줄어들면) 다시 무장한다.
/// - 경고를 켜 두었으면 경고가 최소한 경고 시간만큼 보인 뒤에 잠근다.
///   (예: 긴 시간 규칙에서 짧은 규칙으로 바뀌어 곧바로 시간이 넘었더라도 바로 잠그지 않고 경고부터 띄운다)
pub struct LockScheduler {
    armed: bool,
    last_idle: Option<f64>,
    warning_deadline: Option<Timestamp>,
}

impl Default for LockScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl LockScheduler {
    pub fn new() -> Self {
        LockScheduler { armed: true, last_idle: None, warning_deadline: None }
    }

    /// 실제로 쓰는 경고 시간. 잠금 시간의 절반을 넘지 않게 해서,
    /// 계속 사용 중인데도 경고 단계에 머물다 잠기는 일이 없게 한다.
    pub fn effective_warning(timeout: f64, warning_seconds: f64) -> f64 {
        warning_seconds.min(timeout / 2.0).max(0.0)
    }

    /// 사용자가 다시 움직였다고 알려 준다 (예: 화면 잠금이 풀림).
    pub fn note_user_activity(&mut self) {
        self.armed = true;
        self.warning_deadline = None;
    }

    pub fn tick(&mut self, input: &TickInput) -> TickOutput {
        // 자리비움 시간이 줄었거나 1초 안쪽이면 그 사이 사용자가 입력한 것이다.
        let reset = input.idle_seconds < 1.0 || self.last_idle.is_some_and(|last| input.idle_seconds + 0.5 < last);
        if reset {
            self.armed = true;
            self.warning_deadline = None;
        }
        self.last_idle = Some(input.idle_seconds);

        if input.screen_is_locked {
            self.armed = false;
            self.warning_deadline = None;
            return TickOutput::new(EngineState::ScreenLocked, false);
        }
        if !input.enabled {
            self.warning_deadline = None;
            return TickOutput::new(EngineState::Disabled, false);
        }
        match input.pause {
            Some(Some(until)) if until > input.now => {
                self.warning_deadline = None;
                return TickOutput::new(EngineState::Paused { until: Some(until) }, false);
            }
            Some(None) => {
                self.warning_deadline = None;
                return TickOutput::new(EngineState::Paused { until: None }, false);
            }
            _ => {}
        }
        if input.policy.never_locks() {
            self.warning_deadline = None;
            return TickOutput::new(EngineState::NeverLock, false);
        }
        if !input.assertion_holders.is_empty() {
            self.warning_deadline = None;
            return TickOutput::new(EngineState::HeldByAssertion { holders: input.assertion_holders.clone() }, false);
        }
        if !self.armed {
            return TickOutput::new(EngineState::WaitingForActivity, false);
        }

        let timeout = input.policy.timeout_seconds as f64;
        let remaining = timeout - input.idle_seconds;
        let warning = Self::effective_warning(timeout, input.warning_seconds);

        if input.warn_before_lock && warning > 0.0 {
            if remaining <= warning {
                // 경고는 최소 warning 초 동안 보이게 한다.
                let deadline = *self.warning_deadline.get_or_insert(input.now + remaining.max(warning));
                let left = deadline - input.now;
                if left <= 0.0 {
                    self.armed = false;
                    self.warning_deadline = None;
                    return TickOutput::new(EngineState::LockTriggered, true);
                }
                return TickOutput::new(EngineState::Warning { remaining: left }, false);
            }
            self.warning_deadline = None;
            return TickOutput::new(EngineState::Counting { remaining, timeout }, false);
        }

        if remaining <= 0.0 {
            self.armed = false;
            return TickOutput::new(EngineState::LockTriggered, true);
        }
        TickOutput::new(EngineState::Counting { remaining, timeout }, false)
    }
}
