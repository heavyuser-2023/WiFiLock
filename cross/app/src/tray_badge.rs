//! 윈도우·리눅스 트레이 아이콘용 동적 숫자(남은 시간) 및 잠금 보류(`보류`/`HOLD`) 배지 렌더러.
//!
//! 맥은 메뉴 막대 네이티브 `set_title` 텍스트를 쓰므로 `TrayBadge::Static`만 사용하고,
//! 윈도우·리눅스는 64×64 RGBA 배지를 생성해 트레이 아이콘 자체에 상태를 직접 그린다.

use tauri::image::Image;
use wifilock_core::{AppSettings, EngineState, Lang};

use crate::ui::IconKind;

const SIZE: usize = 64;

/// 숫자 배지 단위. 색·하단 진행 바 스타일 구분에 사용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeUnit {
    Minutes,
    Seconds,
    Hours,
}

/// 트레이 아이콘에 표시할 시각 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayBadge {
    /// 기본 정적 아이콘(`color-*.png` 또는 `mac-*.png`)을 그대로 사용한다.
    Static(IconKind),
    /// 잠금 보류 배지 (`보류` / `HOLD`).
    Held { lang: Lang },
    /// 남은 시간 카운트다운 숫자 배지.
    Countdown {
        text: String,
        unit: BadgeUnit,
        warning: bool,
        /// 하단 진행 바 단계 (0..=16). 값이 바뀔 때만 아이콘을 다시 그린다.
        progress_steps: u8,
    },
}

impl TrayBadge {
    /// 동적 배지를 쓰지 않거나 일반 정적 아이콘이 필요할 때의 기본 `IconKind`.
    pub fn fallback_icon(&self) -> IconKind {
        match self {
            TrayBadge::Static(kind) => *kind,
            TrayBadge::Held { .. } => IconKind::Held,
            TrayBadge::Countdown { warning: true, .. } => IconKind::Warning,
            TrayBadge::Countdown { warning: false, .. } => IconKind::Active,
        }
    }

    /// 엔진 상태·설정·언어·플랫폼 동적 배지 여부로부터 `TrayBadge`를 계산한다.
    pub fn compute(
        state: &EngineState,
        settings: &AppSettings,
        lang: Lang,
        use_dynamic: bool,
    ) -> Self {
        let fallback = fallback_icon_for_state(state);
        if !use_dynamic {
            return TrayBadge::Static(fallback);
        }

        match state {
            EngineState::HeldByAssertion { .. } | EngineState::HeldByBluetooth { .. } => {
                TrayBadge::Held { lang }
            }
            EngineState::Warning { remaining } if settings.show_countdown_in_menu_bar => {
                let secs = (remaining.ceil().max(1.0) as i64).min(99);
                let steps = ((remaining / 10.0).clamp(0.0, 1.0) * 16.0).round() as u8;
                TrayBadge::Countdown {
                    text: secs.to_string(),
                    unit: BadgeUnit::Seconds,
                    warning: true,
                    progress_steps: steps,
                }
            }
            EngineState::Counting { remaining, timeout } if settings.show_countdown_in_menu_bar => {
                let total = remaining.ceil().max(0.0) as i64;
                let timeout_s = timeout.max(1.0);
                let steps = ((remaining / timeout_s).clamp(0.0, 1.0) * 16.0).round() as u8;
                let (text, unit) = format_countdown_badge(total, settings.countdown_in_minutes);
                TrayBadge::Countdown {
                    text,
                    unit,
                    warning: false,
                    progress_steps: steps,
                }
            }
            _ => TrayBadge::Static(fallback),
        }
    }
}

fn fallback_icon_for_state(state: &EngineState) -> IconKind {
    match state {
        EngineState::Warning { .. } => IconKind::Warning,
        EngineState::HeldByAssertion { .. } | EngineState::HeldByBluetooth { .. } => IconKind::Held,
        EngineState::Disabled
        | EngineState::Paused { .. }
        | EngineState::NeverLock
        | EngineState::IdleUnavailable => IconKind::Inactive,
        _ => IconKind::Active,
    }
}

/// 64×64 배지(16×16 축소 시 2글자까지 가장 선명)에 맞게 숫자와 단위를 결정한다.
fn format_countdown_badge(total_secs: i64, in_minutes: bool) -> (String, BadgeUnit) {
    let total = total_secs.max(0);
    if !in_minutes && total <= 99 {
        return (total.to_string(), BadgeUnit::Seconds);
    }
    if in_minutes && total <= 60 {
        return (total.to_string(), BadgeUnit::Seconds);
    }

    let mins = (total + 59) / 60;
    if mins <= 99 {
        return (mins.to_string(), BadgeUnit::Minutes);
    }

    let hours = (mins + 30) / 60;
    if hours <= 9 {
        (format!("{hours}h"), BadgeUnit::Hours)
    } else {
        ("9+".to_string(), BadgeUnit::Hours)
    }
}

/// `TrayBadge`가 동적 배지(`Held` / `Countdown`)이면 64×64 RGBA `Image`를 생성해 반환한다.
/// `TrayBadge::Static`이면 `None`을 반환하므로 호출자가 기본 PNG 아이콘을 사용한다.
pub fn render_badge(badge: &TrayBadge) -> Option<Image<'static>> {
    render_badge_rgba(badge).map(|rgba| Image::new_owned(rgba, SIZE as u32, SIZE as u32))
}

fn render_badge_rgba(badge: &TrayBadge) -> Option<Vec<u8>> {
    match badge {
        TrayBadge::Static(_) => None,
        TrayBadge::Held { lang } => Some(draw_held_badge(*lang)),
        TrayBadge::Countdown {
            text,
            unit,
            warning,
            progress_steps,
        } => Some(draw_countdown_badge(text, *unit, *warning, *progress_steps)),
    }
}

#[derive(Clone, Copy)]
struct Rgba(u8, u8, u8, u8);

struct Canvas {
    px: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Self {
            px: vec![0u8; SIZE * SIZE * 4],
        }
    }

    fn blend(&mut self, x: usize, y: usize, color: Rgba, coverage: f32) {
        if x >= SIZE || y >= SIZE {
            return;
        }
        let a = ((color.3 as f32 / 255.0) * coverage).clamp(0.0, 1.0);
        if a <= 0.001 {
            return;
        }
        let idx = (y * SIZE + x) * 4;
        let dr = self.px[idx] as f32;
        let dg = self.px[idx + 1] as f32;
        let db = self.px[idx + 2] as f32;
        let da = self.px[idx + 3] as f32 / 255.0;

        let out_a = a + da * (1.0 - a);
        if out_a <= 0.0001 {
            return;
        }
        let sr = color.0 as f32;
        let sg = color.1 as f32;
        let sb = color.2 as f32;

        self.px[idx] = ((sr * a + dr * da * (1.0 - a)) / out_a).round().clamp(0.0, 255.0) as u8;
        self.px[idx + 1] = ((sg * a + dg * da * (1.0 - a)) / out_a).round().clamp(0.0, 255.0) as u8;
        self.px[idx + 2] = ((sb * a + db * da * (1.0 - a)) / out_a).round().clamp(0.0, 255.0) as u8;
        self.px[idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
    }

    /// 기존 아이콘(`make_tray_icons.swift`의 x=1..63, y=1..63, radius=14)과 동일한 둥근 사각형 바탕.
    fn fill_rounded_bg(&mut self, bg: Rgba, border: Rgba) {
        let cx = 32.0_f32;
        let cy = 32.0_f32;
        let half = 30.5_f32;
        let radius = 14.0_f32;

        for y in 0..SIZE {
            for x in 0..SIZE {
                let d = sdf_round_rect(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, half, half, radius);
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    // 바깥 테두리 1.2px 살짝 어둡게 눌러 흰 배경/검은 배경 모두에서 경계가 또렷하게 보이도록 처리.
                    let inner_cov = (-0.8 - d).clamp(0.0, 1.0);
                    let border_cov = (cov - inner_cov).max(0.0);
                    if inner_cov > 0.0 {
                        self.blend(x, y, bg, inner_cov);
                    }
                    if border_cov > 0.0 {
                        self.blend(x, y, border, border_cov);
                    }
                }
            }
        }
    }

    fn fill_capsule_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, color: Rgba) {
        let cx = (x0 + x1) * 0.5;
        let cy = (y0 + y1) * 0.5;
        let hx = ((x1 - x0) * 0.5).max(0.5);
        let hy = ((y1 - y0) * 0.5).max(0.5);
        let r = radius.min(hx).min(hy);
        let min_x = (x0 - 1.5).floor().max(0.0) as usize;
        let max_x = (x1 + 1.5).ceil().min((SIZE - 1) as f32) as usize;
        let min_y = (y0 - 1.5).floor().max(0.0) as usize;
        let max_y = (y1 + 1.5).ceil().min((SIZE - 1) as f32) as usize;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let d = sdf_round_rect(x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, hx, hy, r);
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(x, y, color, cov);
                }
            }
        }
    }

    fn stroke_segment(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgba) {
        let r = width * 0.5;
        let min_x = (x0.min(x1) - r - 1.5).floor().max(0.0) as usize;
        let max_x = (x0.max(x1) + r + 1.5).ceil().min((SIZE - 1) as f32) as usize;
        let min_y = (y0.min(y1) - r - 1.5).floor().max(0.0) as usize;
        let max_y = (y0.max(y1) + r + 1.5).ceil().min((SIZE - 1) as f32) as usize;

        let dx = x1 - x0;
        let dy = y1 - y0;
        let len_sq = dx * dx + dy * dy;

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f32 + 0.5 - x0;
                let py = y as f32 + 0.5 - y0;
                let t = if len_sq > 1e-6 {
                    ((px * dx + py * dy) / len_sq).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let dist = ((px - dx * t).powi(2) + (py - dy * t).powi(2)).sqrt() - r;
                let cov = (0.5 - dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend(x, y, color, cov);
                }
            }
        }
    }
}

fn sdf_round_rect(px: f32, py: f32, hx: f32, hy: f32, r: f32) -> f32 {
    let qx = px.abs() - (hx - r);
    let qy = py.abs() - (hy - r);
    let out = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    out + inside - r
}

fn draw_held_badge(lang: Lang) -> Vec<u8> {
    let mut c = Canvas::new();
    // color-held.png 계열 청록 바탕
    c.fill_rounded_bg(Rgba(0, 143, 135, 255), Rgba(0, 102, 96, 255));
    // 상단 상태 강조 바 (민트색) — 일반 숫자 아이콘(하단 진행 바)과 즉시 구별된다.
    c.fill_capsule_rect(12.0, 6.0, 52.0, 11.0, 2.5, Rgba(125, 249, 232, 255));

    let fg = Rgba(255, 255, 255, 255);
    match lang {
        Lang::Ko => draw_korean_held(&mut c, fg),
        Lang::En => draw_english_held(&mut c, fg),
    }
    c.px
}

fn draw_countdown_badge(text: &str, unit: BadgeUnit, warning: bool, progress_steps: u8) -> Vec<u8> {
    let mut c = Canvas::new();
    let (bg, border, track, bar) = if warning {
        (
            Rgba(240, 128, 0, 255),
            Rgba(178, 86, 0, 255),
            Rgba(136, 62, 0, 255),
            Rgba(255, 245, 196, 255),
        )
    } else {
        match unit {
            BadgeUnit::Seconds => (
                Rgba(29, 78, 216, 255),   // 진한 파랑 (초 단위)
                Rgba(20, 52, 155, 255),
                Rgba(15, 38, 114, 255),
                Rgba(250, 204, 21, 255),  // 선명한 노랑 진행 바 (초 단위)
            ),
            BadgeUnit::Minutes | BadgeUnit::Hours => (
                Rgba(46, 112, 237, 255),  // 밝은 파랑 (분/시간 단위, color-active.png 계열)
                Rgba(28, 76, 178, 255),
                Rgba(20, 56, 138, 255),
                Rgba(191, 219, 254, 255), // 연한 하늘색 진행 바
            ),
        }
    };

    c.fill_rounded_bg(bg, border);

    // 하단 진행 바 (x=10..54, y=50..56)
    let track_x0 = 10.0_f32;
    let track_x1 = 54.0_f32;
    c.fill_capsule_rect(track_x0, 50.0, track_x1, 56.0, 3.0, track);
    let ratio = (progress_steps.min(16) as f32) / 16.0;
    let fill_w = (track_x1 - track_x0) * ratio;
    if fill_w >= 5.0 {
        c.fill_capsule_rect(track_x0, 50.0, track_x0 + fill_w, 56.0, 3.0, bar);
    }

    draw_centered_text(&mut c, text, Rgba(255, 255, 255, 255));
    c.px
}

struct GlyphBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    s: f32,
}

fn draw_centered_text(c: &mut Canvas, text: &str, color: Rgba) {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return;
    }

    if chars.len() == 1 {
        let w = 24.0_f32;
        let h = 33.0_f32;
        let box0 = GlyphBox {
            x: (SIZE as f32 - w) * 0.5,
            y: 10.5_f32,
            w,
            h,
            s: 7.0,
        };
        draw_glyph(c, chars[0], box0, color);
        return;
    }

    // 2글자 ("30", "45", "2h", "9+")
    let w = 18.5_f32;
    let h = 31.0_f32;
    let gap = 5.0_f32;
    let total_w = w * 2.0 + gap;
    let start_x = (SIZE as f32 - total_w) * 0.5;
    let y = 11.5_f32;
    draw_glyph(c, chars[0], GlyphBox { x: start_x, y, w, h, s: 5.6 }, color);
    draw_glyph(c, chars[1], GlyphBox { x: start_x + w + gap, y, w, h, s: 5.6 }, color);
}

/// 기하학적 캡슐 선분으로 `0-9`, `h`, `+`를 그린다. 16×16 축소 시에도 뭉개지지 않도록 획을 굵게 유지한다.
fn draw_glyph(c: &mut Canvas, ch: char, box_rect: GlyphBox, color: Rgba) {
    let GlyphBox { x, y, w, h, s } = box_rect;
    let r = x + w;
    let b = y + h;
    let mx = x + w * 0.5;
    let my = y + h * 0.5;
    let seg = |canvas: &mut Canvas, ax: f32, ay: f32, bx: f32, by: f32| {
        canvas.stroke_segment(ax, ay, bx, by, s, color);
    };

    match ch {
        '0' => {
            seg(c, x + 2.0, y, r - 2.0, y);
            seg(c, x + 2.0, b, r - 2.0, b);
            seg(c, x, y + 2.0, x, b - 2.0);
            seg(c, r, y + 2.0, r, b - 2.0);
        }
        '1' => {
            let cx = mx + w * 0.08;
            seg(c, cx, y, cx, b);
            seg(c, x + w * 0.12, y + h * 0.22, cx, y);
            seg(c, x + w * 0.12, b, r - w * 0.05, b);
        }
        '2' => {
            seg(c, x, y, r, y);
            seg(c, r, y, r, my - 1.0);
            seg(c, r, my - 1.0, x, b);
            seg(c, x, b, r, b);
        }
        '3' => {
            seg(c, x, y, r, y);
            seg(c, r, y, r, b);
            seg(c, x + 2.0, my, r, my);
            seg(c, x, b, r, b);
        }
        '4' => {
            seg(c, x, y, x, my + 2.0);
            seg(c, x, my + 2.0, r, my + 2.0);
            seg(c, r - 1.0, y, r - 1.0, b);
        }
        '5' => {
            seg(c, r, y, x, y);
            seg(c, x, y, x, my - 1.0);
            seg(c, x, my - 1.0, r, my - 1.0);
            seg(c, r, my - 1.0, r, b);
            seg(c, r, b, x, b);
        }
        '6' => {
            seg(c, r, y, x, y);
            seg(c, x, y, x, b);
            seg(c, x, my, r, my);
            seg(c, r, my, r, b);
            seg(c, x, b, r, b);
        }
        '7' => {
            seg(c, x, y, r, y);
            seg(c, r, y, x + w * 0.3, b);
        }
        '8' => {
            seg(c, x, y, r, y);
            seg(c, x, my, r, my);
            seg(c, x, b, r, b);
            seg(c, x, y, x, b);
            seg(c, r, y, r, b);
        }
        '9' => {
            seg(c, x, y, r, y);
            seg(c, x, y, x, my);
            seg(c, x, my, r, my);
            seg(c, r, y, r, b);
            seg(c, x, b, r, b);
        }
        'h' => {
            seg(c, x + 1.0, y, x + 1.0, b);
            seg(c, x + 1.0, my - 1.5, r - 1.0, my - 1.5);
            seg(c, r - 1.0, my - 1.5, r - 1.0, b);
        }
        '+' => {
            let half_arm = (w * 0.46).min(h * 0.30);
            seg(c, mx - half_arm, my, mx + half_arm, my);
            seg(c, mx, my - half_arm, mx, my + half_arm);
        }
        _ => {}
    }
}

/// 한국어 잠금 보류: "보류" 2글자를 64×64 안에 굵고 또렷하게 배치한다.
fn draw_korean_held(c: &mut Canvas, color: Rgba) {
    let s = 4.4_f32;
    let seg = |canvas: &mut Canvas, ax: f32, ay: f32, bx: f32, by: f32| {
        canvas.stroke_segment(ax, ay, bx, by, s, color);
    };

    // "보" (왼쪽 x=8..29, y=18..52)
    seg(c, 11.0, 20.0, 11.0, 35.0);
    seg(c, 26.0, 20.0, 26.0, 35.0);
    seg(c, 11.0, 27.5, 26.0, 27.5);
    seg(c, 11.0, 35.0, 26.0, 35.0);
    seg(c, 18.5, 36.0, 18.5, 48.0);
    seg(c, 8.0, 48.0, 29.0, 48.0);

    // "류" (오른쪽 x=35..56, y=18..52)
    seg(c, 37.5, 20.0, 53.5, 20.0);
    seg(c, 53.5, 20.0, 53.5, 26.5);
    seg(c, 37.5, 26.5, 53.5, 26.5);
    seg(c, 37.5, 26.5, 37.5, 33.0);
    seg(c, 37.5, 33.0, 53.5, 33.0);
    seg(c, 35.0, 40.5, 56.0, 40.5);
    seg(c, 41.5, 40.5, 41.5, 52.0);
    seg(c, 49.5, 40.5, 49.5, 52.0);
}

/// 영어 잠금 보류: "HOLD"를 2×2("HO" / "LD")로 크게 배치해 16×16 축소 시에도 읽기 쉽게 그린다.
fn draw_english_held(c: &mut Canvas, color: Rgba) {
    let s = 4.2_f32;
    let seg = |canvas: &mut Canvas, ax: f32, ay: f32, bx: f32, by: f32| {
        canvas.stroke_segment(ax, ay, bx, by, s, color);
    };

    // 윗줄: H (x=11..27, y=16..33), O (x=37..53, y=16..33)
    seg(c, 11.0, 16.5, 11.0, 33.0);
    seg(c, 27.0, 16.5, 27.0, 33.0);
    seg(c, 11.0, 24.5, 27.0, 24.5);

    seg(c, 37.0, 16.5, 53.0, 16.5);
    seg(c, 37.0, 33.0, 53.0, 33.0);
    seg(c, 37.0, 16.5, 37.0, 33.0);
    seg(c, 53.0, 16.5, 53.0, 33.0);

    // 아랫줄: L (x=11..27, y=40..56), D (x=37..53, y=40..56)
    seg(c, 11.0, 40.0, 11.0, 56.0);
    seg(c, 11.0, 56.0, 27.0, 56.0);

    seg(c, 37.0, 40.0, 37.0, 56.0);
    seg(c, 37.0, 40.0, 48.5, 40.0);
    seg(c, 48.5, 40.0, 53.0, 45.0);
    seg(c, 53.0, 45.0, 53.0, 51.0);
    seg(c, 53.0, 51.0, 48.5, 56.0);
    seg(c, 37.0, 56.0, 48.5, 56.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_when_dynamic_disabled() {
        let s = AppSettings {
            show_countdown_in_menu_bar: true,
            ..AppSettings::default()
        };
        let counting = EngineState::Counting {
            remaining: 95.0,
            timeout: 300.0,
        };
        assert_eq!(
            TrayBadge::compute(&counting, &s, Lang::Ko, false),
            TrayBadge::Static(IconKind::Active)
        );
        let held = EngineState::HeldByBluetooth {
            devices: vec!["iPhone".into()],
        };
        assert_eq!(
            TrayBadge::compute(&held, &s, Lang::Ko, false),
            TrayBadge::Static(IconKind::Held)
        );
    }

    #[test]
    fn held_badge_always_shows_even_when_countdown_off() {
        let s = AppSettings {
            show_countdown_in_menu_bar: false,
            ..AppSettings::default()
        };
        let held_app = EngineState::HeldByAssertion {
            holders: vec!["VLC".into()],
        };
        let held_bt = EngineState::HeldByBluetooth {
            devices: vec!["iPhone".into()],
        };
        assert_eq!(
            TrayBadge::compute(&held_app, &s, Lang::Ko, true),
            TrayBadge::Held { lang: Lang::Ko }
        );
        assert_eq!(
            TrayBadge::compute(&held_bt, &s, Lang::En, true),
            TrayBadge::Held { lang: Lang::En }
        );
    }

    #[test]
    fn countdown_minutes_and_seconds_modes() {
        let mut s = AppSettings {
            show_countdown_in_menu_bar: true,
            countdown_in_minutes: true,
            ..AppSettings::default()
        };
        // 분 단위: 30분 (1800초) -> "30" (Minutes)
        assert_eq!(
            TrayBadge::compute(
                &EngineState::Counting {
                    remaining: 1800.0,
                    timeout: 1800.0
                },
                &s,
                Lang::Ko,
                true
            ),
            TrayBadge::Countdown {
                text: "30".into(),
                unit: BadgeUnit::Minutes,
                warning: false,
                progress_steps: 16,
            }
        );

        // 분 단위에서도 마지막 60초 이하는 초 단위로 전환
        assert_eq!(
            TrayBadge::compute(
                &EngineState::Counting {
                    remaining: 44.2,
                    timeout: 300.0
                },
                &s,
                Lang::Ko,
                true
            ),
            TrayBadge::Countdown {
                text: "45".into(),
                unit: BadgeUnit::Seconds,
                warning: false,
                progress_steps: 2,
            }
        );

        // 초 단위 모드: <= 99초는 초 표시, > 99초는 분 올림
        s.countdown_in_minutes = false;
        assert_eq!(
            TrayBadge::compute(
                &EngineState::Counting {
                    remaining: 95.0,
                    timeout: 300.0
                },
                &s,
                Lang::Ko,
                true
            ),
            TrayBadge::Countdown {
                text: "95".into(),
                unit: BadgeUnit::Seconds,
                warning: false,
                progress_steps: 5,
            }
        );
        assert_eq!(
            TrayBadge::compute(
                &EngineState::Counting {
                    remaining: 105.0,
                    timeout: 300.0
                },
                &s,
                Lang::Ko,
                true
            ),
            TrayBadge::Countdown {
                text: "2".into(),
                unit: BadgeUnit::Minutes,
                warning: false,
                progress_steps: 6,
            }
        );

        // 99분 초과 -> 시간 단위("2h" 등)
        assert_eq!(
            TrayBadge::compute(
                &EngineState::Counting {
                    remaining: 7200.0,
                    timeout: 7200.0
                },
                &s,
                Lang::En,
                true
            ),
            TrayBadge::Countdown {
                text: "2h".into(),
                unit: BadgeUnit::Hours,
                warning: false,
                progress_steps: 16,
            }
        );
    }

    #[test]
    fn warning_badge_and_pixel_rendering() {
        let s = AppSettings {
            show_countdown_in_menu_bar: true,
            ..AppSettings::default()
        };
        let badge = TrayBadge::compute(&EngineState::Warning { remaining: 9.2 }, &s, Lang::Ko, true);
        assert_eq!(
            badge,
            TrayBadge::Countdown {
                text: "10".into(),
                unit: BadgeUnit::Seconds,
                warning: true,
                progress_steps: 15,
            }
        );

        assert!(render_badge(&TrayBadge::Static(IconKind::Active)).is_none());

        for b in [
            badge,
            TrayBadge::Held { lang: Lang::Ko },
            TrayBadge::Held { lang: Lang::En },
            TrayBadge::Countdown {
                text: "30".into(),
                unit: BadgeUnit::Minutes,
                warning: false,
                progress_steps: 12,
            },
            TrayBadge::Countdown {
                text: "2h".into(),
                unit: BadgeUnit::Hours,
                warning: false,
                progress_steps: 16,
            },
        ] {
            let rgba = render_badge_rgba(&b).expect("동적 배지는 RGBA를 생성해야 한다");
            assert_eq!(rgba.len(), SIZE * SIZE * 4);
            // 코너 (0,0)은 투명, 중앙 부근에 흰색 글자 픽셀이 존재해야 한다.
            assert_eq!(rgba[3], 0);
            let has_white = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[0] >= 250 && p[1] >= 250 && p[2] >= 250 && p[3] == 255);
            assert!(has_white, "배지 {b:?} 에 흰색 글자가 그려져야 한다");
        }
    }

    /// 시각적 품질 점검용 raw RGBA 덤프 (스크래치 폴더에 프리뷰 시트를 만들 때 `--ignored`로 실행).
    #[test]
    #[ignore]
    fn dump_preview_raw_badges() {
        let out_dir = std::path::Path::new(
            "/Users/heavyuser/.gemini/antigravity/brain/d5a973c3-f510-4e2f-9abb-01504dd36865/scratch/badges",
        );
        std::fs::create_dir_all(out_dir).expect("배지 디렉터리 생성");
        let samples = [
            (
                "min_30",
                TrayBadge::Countdown {
                    text: "30".into(),
                    unit: BadgeUnit::Minutes,
                    warning: false,
                    progress_steps: 14,
                },
            ),
            (
                "min_5",
                TrayBadge::Countdown {
                    text: "5".into(),
                    unit: BadgeUnit::Minutes,
                    warning: false,
                    progress_steps: 4,
                },
            ),
            (
                "sec_45",
                TrayBadge::Countdown {
                    text: "45".into(),
                    unit: BadgeUnit::Seconds,
                    warning: false,
                    progress_steps: 10,
                },
            ),
            (
                "warn_10",
                TrayBadge::Countdown {
                    text: "10".into(),
                    unit: BadgeUnit::Seconds,
                    warning: true,
                    progress_steps: 15,
                },
            ),
            (
                "hour_2h",
                TrayBadge::Countdown {
                    text: "2h".into(),
                    unit: BadgeUnit::Hours,
                    warning: false,
                    progress_steps: 16,
                },
            ),
            ("held_ko", TrayBadge::Held { lang: Lang::Ko }),
            ("held_en", TrayBadge::Held { lang: Lang::En }),
        ];
        for (name, badge) in samples {
            let rgba = render_badge_rgba(&badge).expect("동적 배지");
            std::fs::write(out_dir.join(format!("{name}.rgba")), rgba).expect("raw rgba 쓰기");
        }
    }
}
