//! Token usage with explicit provenance (`docs/TOKEN-INTELLIGENCE.md` §2–§3).

use serde::{Deserialize, Serialize};

/// Where a usage number came from. A number is never `Reported` unless parsed from provider output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageProvenance {
    Reported,
    Estimated,
    Unavailable,
}

/// Token counts for one agent run (or one turn).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenUsage {
    /// Uncached input tokens.
    pub input: u64,
    pub cached_input_read: Option<u64>,
    pub cached_input_write: Option<u64>,
    pub output: u64,
    pub reasoning: Option<u64>,
}

impl TokenUsage {
    /// All tokens processed, including cache reads and writes.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.input
            + self.cached_input_read.unwrap_or(0)
            + self.cached_input_write.unwrap_or(0)
            + self.output
            + self.reasoning.unwrap_or(0)
    }
}

impl std::ops::Add for TokenUsage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        fn opt(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            match (a, b) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            }
        }
        Self {
            input: self.input + rhs.input,
            cached_input_read: opt(self.cached_input_read, rhs.cached_input_read),
            cached_input_write: opt(self.cached_input_write, rhs.cached_input_write),
            output: self.output + rhs.output,
            reasoning: opt(self.reasoning, rhs.reasoning),
        }
    }
}

/// A usage value whose provenance is part of the type: unknown values cannot be shown as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provenance", rename_all = "snake_case")]
pub enum UsageValue {
    Reported { usage: TokenUsage },
    Estimated { usage: TokenUsage },
    Unavailable,
}

impl UsageValue {
    #[must_use]
    pub const fn provenance(&self) -> UsageProvenance {
        match self {
            Self::Reported { .. } => UsageProvenance::Reported,
            Self::Estimated { .. } => UsageProvenance::Estimated,
            Self::Unavailable => UsageProvenance::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_counts_cache_and_reasoning() {
        let u = TokenUsage {
            input: 10,
            cached_input_read: Some(10_362),
            cached_input_write: Some(3_761),
            output: 389,
            reasoning: None,
        };
        assert_eq!(u.total(), 14_522);
    }

    #[test]
    fn add_keeps_none_when_both_none() {
        let a = TokenUsage {
            input: 1,
            output: 2,
            ..TokenUsage::default()
        };
        let b = TokenUsage {
            input: 3,
            cached_input_read: Some(5),
            output: 4,
            ..TokenUsage::default()
        };
        let sum = a + b;
        assert_eq!(sum.input, 4);
        assert_eq!(sum.cached_input_read, Some(5));
        assert_eq!(sum.reasoning, None);
    }

    #[test]
    fn provenance_tag_in_json() {
        let json = serde_json::to_string(&UsageValue::Unavailable).expect("serialize");
        assert_eq!(json, r#"{"provenance":"unavailable"}"#);
    }
}
