//! Job identity and workflow states (see `docs/WORKFLOW.md` §2–§3).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Human-readable job key, e.g. `API-42`: project key prefix + per-project sequence number.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobKey {
    prefix: String,
    number: u32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JobKeyError {
    #[error("job key must look like `PREFIX-<number>`, e.g. `API-42`")]
    Format,
    #[error("key prefix must be 2–6 uppercase ASCII letters")]
    Prefix,
    #[error("job number must be a positive integer")]
    Number,
}

impl JobKey {
    pub fn new(prefix: impl Into<String>, number: u32) -> Result<Self, JobKeyError> {
        let prefix = prefix.into();
        if !(2..=6).contains(&prefix.len()) || !prefix.bytes().all(|b| b.is_ascii_uppercase()) {
            return Err(JobKeyError::Prefix);
        }
        if number == 0 {
            return Err(JobKeyError::Number);
        }
        Ok(Self { prefix, number })
    }

    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    #[must_use]
    pub const fn number(&self) -> u32 {
        self.number
    }
}

impl fmt::Display for JobKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.prefix, self.number)
    }
}

impl FromStr for JobKey {
    type Err = JobKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (prefix, number) = s.split_once('-').ok_or(JobKeyError::Format)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(JobKeyError::Number);
        }
        let number: u32 = number.parse().map_err(|_| JobKeyError::Number)?;
        Self::new(prefix, number)
    }
}

impl Serialize for JobKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for JobKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// Stages of the job state machine. Holds (pause, budget wait, rate limit) are overlays, not states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Backlog,
    Discovery,
    Specification,
    ScopeApproval,
    Planning,
    PlanApproval,
    Implementation,
    BuildTest,
    Review,
    Verification,
    AcceptanceApproval,
    Delivery,
    AwaitingRelease,
    Done,
    Blocked,
    Cancelled,
    Failed,
}

impl JobState {
    pub const ALL: [Self; 17] = [
        Self::Backlog,
        Self::Discovery,
        Self::Specification,
        Self::ScopeApproval,
        Self::Planning,
        Self::PlanApproval,
        Self::Implementation,
        Self::BuildTest,
        Self::Review,
        Self::Verification,
        Self::AcceptanceApproval,
        Self::Delivery,
        Self::AwaitingRelease,
        Self::Done,
        Self::Blocked,
        Self::Cancelled,
        Self::Failed,
    ];

    /// Terminal states are absorbing (invariant J-6).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Cancelled | Self::Failed)
    }

    /// States that wait for a human decision.
    #[must_use]
    pub const fn is_approval(self) -> bool {
        matches!(
            self,
            Self::ScopeApproval | Self::PlanApproval | Self::AcceptanceApproval
        )
    }

    /// From `implementation` onwards a job owns a worktree (invariant J-3).
    #[must_use]
    pub const fn has_worktree(self) -> bool {
        matches!(
            self,
            Self::Implementation
                | Self::BuildTest
                | Self::Review
                | Self::Verification
                | Self::AcceptanceApproval
                | Self::Delivery
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobType {
    Feature,
    Bugfix,
    Refactor,
    Docs,
    Chore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn job_key_parse_and_display() {
        let key: JobKey = "API-42".parse().expect("valid key");
        assert_eq!(key.prefix(), "API");
        assert_eq!(key.number(), 42);
        assert_eq!(key.to_string(), "API-42");
    }

    #[test]
    fn job_key_rejects_invalid() {
        assert_eq!("API42".parse::<JobKey>(), Err(JobKeyError::Format));
        assert_eq!("api-1".parse::<JobKey>(), Err(JobKeyError::Prefix));
        assert_eq!("A-1".parse::<JobKey>(), Err(JobKeyError::Prefix));
        assert_eq!("API-0".parse::<JobKey>(), Err(JobKeyError::Number));
        assert_eq!("API-+1".parse::<JobKey>(), Err(JobKeyError::Number));
        assert_eq!("API-".parse::<JobKey>(), Err(JobKeyError::Number));
    }

    #[test]
    fn terminal_states_are_exactly_three() {
        let terminal: Vec<_> = JobState::ALL.iter().filter(|s| s.is_terminal()).collect();
        assert_eq!(terminal.len(), 3);
    }

    #[test]
    fn state_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&JobState::AcceptanceApproval).expect("serialize"),
            "\"acceptance_approval\""
        );
    }

    proptest! {
        #[test]
        fn job_key_round_trips(prefix in "[A-Z]{2,6}", number in 1u32..) {
            let key = JobKey::new(prefix, number).expect("valid");
            let parsed: JobKey = key.to_string().parse().expect("round trip");
            prop_assert_eq!(parsed, key);
        }
    }
}
