//! Agent roles, complexity classes, trust levels and actors.

use serde::{Deserialize, Serialize};

use crate::RunId;

/// Purpose of an agent run (see `docs/AGENT-PROTOCOL.md` §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Discovery,
    SpecWriter,
    Planner,
    Implementer,
    Fixer,
    Reviewer,
    Verifier,
    Summarizer,
    ReleaseNotes,
}

impl Role {
    /// Roles that may modify files in the worktree.
    #[must_use]
    pub const fn writes_files(self) -> bool {
        matches!(self, Self::Implementer | Self::Fixer)
    }
}

/// Job/task complexity class derived by the deterministic scorer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Complexity {
    XS,
    S,
    M,
    L,
    XL,
}

impl Complexity {
    /// Maps a 0–100 score to a class (`docs/TOKEN-INTELLIGENCE.md` §5).
    #[must_use]
    pub const fn from_score(score: u8) -> Self {
        match score {
            0..10 => Self::XS,
            10..25 => Self::S,
            25..50 => Self::M,
            50..75 => Self::L,
            _ => Self::XL,
        }
    }
}

/// Trust level of content placed into prompts (SEC-010).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    Trusted,
    Internal,
    Untrusted,
}

/// Who caused an event (invariant I-7).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// A human user. The MVP has a single local user with id `local`.
    User {
        id: String,
    },
    System,
    Scheduler,
    AgentRun {
        id: RunId,
    },
}

impl Actor {
    #[must_use]
    pub fn local_user() -> Self {
        Self::User {
            id: "local".to_owned(),
        }
    }

    /// Only users may decide approvals (invariant AP-2).
    #[must_use]
    pub const fn may_decide_approvals(&self) -> bool {
        matches!(self, Self::User { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complexity_boundaries() {
        assert_eq!(Complexity::from_score(0), Complexity::XS);
        assert_eq!(Complexity::from_score(9), Complexity::XS);
        assert_eq!(Complexity::from_score(10), Complexity::S);
        assert_eq!(Complexity::from_score(41), Complexity::M);
        assert_eq!(Complexity::from_score(74), Complexity::L);
        assert_eq!(Complexity::from_score(75), Complexity::XL);
        assert_eq!(Complexity::from_score(255), Complexity::XL);
    }

    #[test]
    fn only_users_decide_approvals() {
        assert!(Actor::local_user().may_decide_approvals());
        assert!(!Actor::System.may_decide_approvals());
        assert!(!Actor::AgentRun { id: RunId::new() }.may_decide_approvals());
    }

    #[test]
    fn actor_serializes_tagged() {
        let json = serde_json::to_string(&Actor::local_user()).expect("serialize");
        assert_eq!(json, r#"{"kind":"user","id":"local"}"#);
    }
}
