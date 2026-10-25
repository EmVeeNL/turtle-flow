//! The services every command works through, wired the way the daemon wires them.
//!
//! Until the HTTP API exists (M4) the CLI opens the database directly. That is safe: every write
//! is an immediate transaction with optimistic job versions, and effects are only ever executed
//! by the daemon, which owns the dispatcher.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Serialize;
use turtle_artifacts::BlobStore;
use turtle_scheduler::{
    ApprovalService, ContextConfig, CrashPlan, Engine, ExecConfig, RepoGitFacts, SharedStore,
    StoreContext,
};
use turtle_store::{Store, SystemClock};

use crate::paths::Home;

pub type Ctx = StoreContext<RepoGitFacts>;

#[derive(Debug)]
pub struct Runtime {
    pub store: SharedStore,
    pub blobs: Arc<BlobStore>,
    pub cfg: Arc<ExecConfig>,
    pub ctx: Arc<Ctx>,
    pub engine: Engine<Ctx>,
    pub approvals: ApprovalService<Ctx, RepoGitFacts>,
}

impl Runtime {
    pub fn open(home: Home) -> Result<Self> {
        home.ensure()?;
        let store = SharedStore::new(
            Store::open(&home.db(), Arc::new(SystemClock))
                .with_context(|| format!("opening {}", home.db().display()))?,
        );
        let blobs = Arc::new(BlobStore::open(home.blobs()).context("opening the blob store")?);
        let cfg = Arc::new(ExecConfig::new(home.worktrees()));
        let git = Arc::new(RepoGitFacts::new(store.clone(), Arc::clone(&cfg)));
        let ctx = Arc::new(StoreContext::new(
            store.clone(),
            Arc::clone(&blobs),
            Arc::clone(&git),
            ContextConfig::default(),
        ));
        let engine = Engine::new(store.clone(), Arc::clone(&ctx), CrashPlan::never());
        let approvals = ApprovalService::new(engine.clone(), Arc::clone(&git));
        Ok(Self {
            store,
            blobs,
            cfg,
            ctx,
            engine,
            approvals,
        })
    }
}

/// The snake_case name a domain enum serializes to (`JobState::BuildTest` becomes `build_test`).
#[must_use]
pub fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".to_owned())
}

/// Parses a snake_case enum name given on the command line.
pub fn parse_label<T: serde::de::DeserializeOwned>(what: &str, text: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| anyhow::anyhow!("unknown {what} `{text}`"))
}
