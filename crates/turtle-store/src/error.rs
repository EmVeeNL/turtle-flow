use turtle_domain::{EffectId, JobId};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("time formatting error: {0}")]
    Time(#[from] time::error::Format),
    /// Optimistic concurrency failure: someone else changed the job first.
    #[error("job {job} was modified concurrently (expected version {expected})")]
    VersionConflict { job: JobId, expected: u64 },
    /// The effect is no longer claimed by this instance (restart, recovery or cancellation).
    #[error("lease on effect {effect} was lost")]
    LeaseLost { effect: EffectId },
    #[error("approval {id} is {status}, not pending")]
    NotPending { id: String, status: String },
    /// The decision echoed a different hash than the one the approval is bound to: the approver
    /// was looking at something other than what would be approved (invariant AP-1).
    #[error("the approval subject changed; nothing was approved")]
    HashMismatch,
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("a comment is required to request changes or reject")]
    CommentRequired,
    #[error("not found: {0}")]
    NotFound(String),
    #[error(
        "database schema version {found} is newer than this build supports ({supported}); upgrade Turtle Flow"
    )]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("stored data is inconsistent: {0}")]
    Corrupt(String),
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;
