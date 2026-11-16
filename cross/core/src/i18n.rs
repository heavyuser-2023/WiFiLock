//! 다국어(한국어·영어) 지원 상태 및 로케일 판별.

use std::sync::atomic::{AtomicU8, Ordering};

use serde::Serialize;

/// 앱 표시 언어. 시스템 설정이 한국어면 `Ko`, 그 외에는 `En` 을 사용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Ko,
    En,
}

impl Lang {
    pub const fn is_ko(self) -> bool {
        matches!(self, Lang::Ko)
    }

    pub const fn code(self) -> &'static str {
        match self {
            Lang::Ko => "ko",
            Lang::En => "en",
        }
    }

    pub fn pick<T>(self, ko: T, en: T) -> T {
        match self {
            Lang::Ko => ko,
            Lang::En => en,
        }
    }
}

/// 단위 테스트가 실행 환경(영문 CI 등)에 영향을 받지 않도록 초기 기본값은 한국어(0)로 둔다.
/// 실제 앱 시작 시 시스템 로케일을 읽어 `set(...)` 으로 덮어쓴다.
static CURRENT_LANG: AtomicU8 = AtomicU8::new(0);

pub fn current() -> Lang {
    match CURRENT_LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Ko,
    }
}

pub fn set(lang: Lang) {
    let v = match lang {
        Lang::Ko => 0,
        Lang::En => 1,
    };
    CURRENT_LANG.store(v, Ordering::Relaxed);
}

pub fn is_ko() -> bool {
    current().is_ko()
}

pub fn code() -> &'static str {
    current().code()
}

pub fn pick<T>(ko: T, en: T) -> T {
    current().pick(ko, en)
}

/// 로케일 문자열("ko_KR.UTF-8", "ko-Kore-KR", "en-US", "ja_JP", "C" 등)을 언어로 해석한다.
/// 비어 있으면 `None`.
pub fn parse_locale_tag(tag: &str) -> Option<Lang> {
    let clean = tag.trim();
    if clean.is_empty() {
        return None;
    }
    let without_enc = clean.split(['.', '@']).next().unwrap_or("").trim();
    if without_enc.is_empty() {
        return None;
    }
    let primary = without_enc
        .split(['_', '-'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if primary.is_empty() {
        return None;
    }
    if primary == "ko" || primary == "kor" || primary == "korean" {
        Some(Lang::Ko)
    } else {
        Some(Lang::En)
    }
}

/// 환경 변수(`WIFILOCK_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`)에서 언어를 추측한다.
pub fn detect_from_env() -> Option<Lang> {
    if let Ok(v) = std::env::var("WIFILOCK_LANG") {
        if let Some(lang) = parse_locale_tag(&v) {
            return Some(lang);
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES"] {
        if let Ok(v) = std::env::var(key) {
            if let Some(lang) = parse_locale_tag(&v) {
                return Some(lang);
            }
        }
    }
    if let Ok(v) = std::env::var("LANGUAGE") {
        for part in v.split(':') {
            if let Some(lang) = parse_locale_tag(part) {
                return Some(lang);
            }
        }
    }
    if let Ok(v) = std::env::var("LANG") {
        if let Some(lang) = parse_locale_tag(&v) {
            return Some(lang);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_tag_parsing() {
        assert_eq!(parse_locale_tag("ko"), Some(Lang::Ko));
        assert_eq!(parse_locale_tag("ko_KR.UTF-8"), Some(Lang::Ko));
        assert_eq!(parse_locale_tag("ko-Kore-KR"), Some(Lang::Ko));
        assert_eq!(parse_locale_tag("KO-KR"), Some(Lang::Ko));
        assert_eq!(parse_locale_tag("Korean"), Some(Lang::Ko));
        assert_eq!(parse_locale_tag("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(parse_locale_tag("en-GB"), Some(Lang::En));
        assert_eq!(parse_locale_tag("ja_JP.UTF-8"), Some(Lang::En));
        assert_eq!(parse_locale_tag("C.UTF-8"), Some(Lang::En));
        assert_eq!(parse_locale_tag("POSIX"), Some(Lang::En));
        assert_eq!(parse_locale_tag(""), None);
        assert_eq!(parse_locale_tag("   "), None);
    }
}
