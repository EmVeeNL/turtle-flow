//! Provider quota observations. Claude Code reports per-window utilization (spike S-1);
//! Codex only reports a limit error. Absolute remaining tokens are never known.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowStatus {
    Allowed,
    /// Utilization passed the provider's warning threshold.
    Warning,
    Rejected,
}

/// One provider usage window, e.g. `five_hour` or `seven_day`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub name: String,
    /// Fraction of the window used, 0.0–1.0 (not tokens).
    pub utilization: f64,
    /// Unix seconds when the window resets, if reported.
    pub resets_at: Option<i64>,
    pub status: WindowStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "visibility", rename_all = "snake_case")]
pub enum QuotaStatus {
    /// Utilization per window is known; absolute remaining tokens are not.
    Partial {
        windows: Vec<QuotaWindow>,
    },
    /// The provider refused work; `retry_at` is unix seconds if it could be derived.
    Limited {
        retry_at: Option<i64>,
        message: String,
    },
    Unknown,
}

impl QuotaStatus {
    /// Highest utilization across known windows, if any.
    #[must_use]
    pub fn max_utilization(&self) -> Option<f64> {
        match self {
            Self::Partial { windows } => windows.iter().map(|w| w.utilization).reduce(f64::max),
            Self::Limited { .. } => Some(1.0),
            Self::Unknown => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_utilization() {
        let q = QuotaStatus::Partial {
            windows: vec![
                QuotaWindow {
                    name: "five_hour".into(),
                    utilization: 0.65,
                    resets_at: Some(1_792_848_600),
                    status: WindowStatus::Allowed,
                },
                QuotaWindow {
                    name: "seven_day".into(),
                    utilization: 0.76,
                    resets_at: Some(1_793_109_600),
                    status: WindowStatus::Warning,
                },
            ],
        };
        assert_eq!(q.max_utilization(), Some(0.76));
        assert_eq!(QuotaStatus::Unknown.max_utilization(), None);
    }
}
