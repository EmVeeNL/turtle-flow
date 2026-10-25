//! Codex adapter pieces verified in spike S-2 (Codex CLI 0.163.0, error path only so far).

use serde_json::Value;
use time::{OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};

/// Flags every headless `codex exec` run must carry (config isolation, no session files).
pub const ISOLATION_FLAGS: &[&str] = &["exec", "--json", "--ephemeral", "--ignore-user-config"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodexError {
    /// Account usage limit; `retry_at` is the local wall-clock time from the message, if present.
    UsageLimit {
        retry_at: Option<(u8, u8)>,
        message: String,
    },
    Other {
        message: String,
    },
}

/// Classifies a JSONL `error` / `turn.failed` event.
#[must_use]
pub fn classify_error_event(line: &str) -> Option<CodexError> {
    let v: Value = serde_json::from_str(line).ok()?;
    let message = match v["type"].as_str()? {
        "error" => v["message"].as_str()?,
        "turn.failed" => v["error"]["message"].as_str()?,
        _ => return None,
    }
    .to_owned();
    if message.contains("hit your usage limit") {
        Some(CodexError::UsageLimit {
            retry_at: parse_retry_time(&message),
            message,
        })
    } else {
        Some(CodexError::Other { message })
    }
}

/// Extracts `try again at 3:36 PM` as 24-hour (hour, minute). The message has no date or zone.
#[must_use]
pub fn parse_retry_time(message: &str) -> Option<(u8, u8)> {
    let rest = &message[message.find("try again at ")? + "try again at ".len()..];
    let mut words = rest.split_whitespace();
    let clock = words.next()?;
    let meridiem = words
        .next()
        .map(|w| w.trim_end_matches('.').to_ascii_uppercase());
    let (h, m) = clock.split_once(':')?;
    let mut hour: u8 = h.parse().ok()?;
    let minute: u8 = m.trim_end_matches('.').parse().ok()?;
    match meridiem.as_deref() {
        Some("PM") if hour < 12 => hour += 12,
        Some("AM") if hour == 12 => hour = 0,
        _ => {}
    }
    (hour < 24 && minute < 60).then_some((hour, minute))
}

/// The machine's UTC offset. `time` refuses to read the local zone once a Unix process is
/// multi-threaded (the daemon always is), so the first lookup falls back to asking `date`, and
/// the answer is cached. `None` means the offset could not be determined at all.
#[must_use]
pub fn local_offset() -> Option<UtcOffset> {
    static OFFSET: std::sync::OnceLock<Option<UtcOffset>> = std::sync::OnceLock::new();
    *OFFSET.get_or_init(|| {
        UtcOffset::current_local_offset().ok().or_else(|| {
            let out = std::process::Command::new("date")
                .arg("+%z")
                .output()
                .ok()?;
            parse_date_offset(&String::from_utf8_lossy(&out.stdout))
        })
    })
}

/// Parses the output of `date +%z`, such as `+0200` or `-0530`.
fn parse_date_offset(text: &str) -> Option<UtcOffset> {
    let text = text.trim();
    let (sign, digits) = match text.split_at_checked(1)? {
        ("+", d) => (1_i8, d),
        ("-", d) => (-1_i8, d),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i8 = digits[..2].parse().ok()?;
    let minutes: i8 = digits[2..].parse().ok()?;
    UtcOffset::from_hms(sign * hours, sign * minutes, 0).ok()
}

/// Resolves a provider's local wall-clock hint to its next occurrence. The caller supplies the
/// daemon clock and local offset, which keeps date-boundary behavior deterministic in tests.
#[must_use]
pub fn retry_instant(
    now_utc: OffsetDateTime,
    local_offset: UtcOffset,
    hour: u8,
    minute: u8,
) -> Option<OffsetDateTime> {
    let local_now = now_utc.to_offset(local_offset);
    let time = Time::from_hms(hour, minute, 0).ok()?;
    let local = PrimitiveDateTime::new(local_now.date(), time).assume_offset(local_offset);
    let candidate = if local > now_utc {
        local
    } else {
        let tomorrow = local_now.date().next_day()?;
        PrimitiveDateTime::new(tomorrow, time).assume_offset(local_offset)
    };
    Some(candidate.to_offset(UtcOffset::UTC))
}

#[cfg(test)]
mod offset_tests {
    use super::*;

    #[test]
    fn date_offsets_parse_and_garbage_does_not() {
        assert_eq!(
            parse_date_offset("+0200\n"),
            UtcOffset::from_hms(2, 0, 0).ok()
        );
        assert_eq!(
            parse_date_offset("-0530"),
            UtcOffset::from_hms(-5, -30, 0).ok()
        );
        assert_eq!(parse_date_offset("+0000"), Some(UtcOffset::UTC));
        for bad in ["", "0200", "+02", "+02:00", "+abcd", "UTC", "+9999"] {
            assert_eq!(parse_date_offset(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_local_offset_is_found_even_in_a_multithreaded_process() {
        let found =
            std::thread::scope(|s| s.spawn(|| local_offset().is_some()).join().expect("thread"));
        assert!(
            found,
            "a machine with a working `date` must yield an offset"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Date, Month};

    #[test]
    fn classifies_recorded_usage_limit() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../spikes/fixtures/codex/01-usage-limit-error.ndjson"
        );
        let text = std::fs::read_to_string(path).expect("fixture exists");
        let errors: Vec<_> = text.lines().filter_map(classify_error_event).collect();
        assert_eq!(errors.len(), 2, "error + turn.failed");
        assert!(matches!(
            &errors[0],
            CodexError::UsageLimit {
                retry_at: Some((15, 36)),
                ..
            }
        ));
    }

    #[test]
    fn retry_time_edge_cases() {
        assert_eq!(parse_retry_time("try again at 12:05 AM."), Some((0, 5)));
        assert_eq!(parse_retry_time("try again at 12:30 PM."), Some((12, 30)));
        assert_eq!(parse_retry_time("try again at 9:00"), Some((9, 0)));
        assert_eq!(parse_retry_time("try again later"), None);
        assert_eq!(parse_retry_time("no hint"), None);
    }

    #[test]
    fn retry_hint_resolves_to_next_occurrence_in_injected_local_time() {
        let now = Date::from_calendar_date(2026, Month::October, 25)
            .expect("valid test date")
            .with_time(Time::from_hms(13, 30, 0).expect("valid test time"))
            .assume_utc();
        let offset = UtcOffset::from_hms(2, 0, 0).expect("valid offset");
        let same_day = retry_instant(now, offset, 16, 0).expect("same day");
        assert_eq!(same_day.hour(), 14);
        assert_eq!(same_day.day(), 25);
        let next_day = retry_instant(now, offset, 14, 0).expect("next day");
        assert!(next_day > now);
        assert_eq!(next_day.to_offset(offset).date().day(), 26);
    }

    #[test]
    fn ignores_non_error_events() {
        assert_eq!(classify_error_event(r#"{"type":"turn.started"}"#), None);
        assert_eq!(classify_error_event("not json"), None);
    }
}
