//! Codex adapter pieces verified in spike S-2 (Codex CLI 0.163.0, error path only so far).

use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ignores_non_error_events() {
        assert_eq!(classify_error_event(r#"{"type":"turn.started"}"#), None);
        assert_eq!(classify_error_event("not json"), None);
    }
}
