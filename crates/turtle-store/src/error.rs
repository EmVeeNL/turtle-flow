use turtle_domain::JobId;

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
