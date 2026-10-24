//! Typed ULID identifiers. Each entity gets its own newtype so IDs cannot be mixed up.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use ulid::Ulid;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Ulid);

        impl $name {
            /// Generates a new, time-sortable ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::generate())
            }

            /// Wraps an existing ULID (e.g. loaded from storage).
            #[must_use]
            pub const fn from_ulid(ulid: Ulid) -> Self {
                Self(ulid)
            }

            #[must_use]
            pub const fn as_ulid(&self) -> Ulid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = ulid::DecodeError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ulid::from_string(s).map(Self)
            }
        }
    };
}

define_id!(
    /// Identifies a project (one Git repository).
    ProjectId
);
define_id!(
    /// Identifies a job.
    JobId
);
define_id!(
    /// Identifies a task within a job's plan.
    TaskId
);
define_id!(
    /// Identifies one agent run (one provider process invocation).
    RunId
);
define_id!(
    /// Identifies an approval.
    ApprovalId
);
define_id!(
    /// Identifies an artifact (all revisions share it).
    ArtifactId
);
define_id!(
    /// Identifies a budget.
    BudgetId
);
define_id!(
    /// Identifies a budget reservation.
    ReservationId
);
define_id!(
    /// Identifies an effect in the outbox.
    EffectId
);
define_id!(
    /// Identifies an event in the event log.
    EventId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let id = JobId::new();
        let parsed: JobId = id.to_string().parse().expect("valid ULID");
        assert_eq!(id, parsed);
        assert_eq!(id.to_string().len(), 26);
    }

    #[test]
    fn serde_is_transparent_string() {
        let id = RunId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, format!("\"{id}\""));
        let back: RunId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn rejects_invalid_ulid() {
        assert!("not-a-ulid".parse::<ProjectId>().is_err());
    }
}
