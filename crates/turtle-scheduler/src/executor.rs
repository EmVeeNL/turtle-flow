//! The real effect executor: turns each queued effect into actions on Git, the agent runner,
//! the project's gate commands, approvals and delivery.
//!
//! Effects are delivered at least once, so every effect starts by probing the real world for
//! what an interrupted earlier attempt may already have done:
//!
//! | effect | probe |
//! |---|---|
//! | `create_worktree` | worktree exists on the expected branch at the recorded base |
//! | `dispatch_ready_tasks` | a commit with the task's `Turtle-Task` trailer exists |
//! | `run_gates`, `run_verification` | none needed: re-running is harmless |
//! | `request_approval` | an identical pending approval exists |
//! | `deliver` | a commit with the job's `Turtle-Job` trailer is on the target, or the parked ref |
//! | `cleanup_worktree` | removing a missing worktree succeeds |

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use turtle_agents::{
    AgentContext, AgentError, AgentOutput, AgentRequest, AgentRunner, Independence, RouteContext,
};
use turtle_artifacts::BlobStore;
use turtle_domain::paths::within_areas;
use turtle_domain::{
    Actor, ApprovalStatus, ArtifactKind, DeliveryMode, GateKind, Plan, QuotaStatus, ReviewReport,
    Role, RunId, Severity, Specification, UsageValue, VerificationDecision, VerificationReport,
};
use turtle_exec::{CommandSpec, ExitOutcome};
use turtle_git::commit::Trailers;
use turtle_git::delivery::{DeliveryOutcome, SquashRequest};
use turtle_git::refresh::RefreshOutcome;
use turtle_git::worktree::WorktreeOutcome;
use turtle_git::{GitError, Repo};
use turtle_store::{
    DeliveryStatus, EffectRow, JobRecord, NewEvent, NewRevision, NewRoutingDecision, NewRun,
    ProjectRecord, ReservationOutcome, RevisionRecord, RunEnd,
};
use turtle_tokens::{
    BucketPath, cold_start_estimate, estimate, reservation_size, score_complexity,
};
use turtle_workflow::{
    BlockReason, Effect, JobCommand, ProfileBase, ReviewIndependence, ReviewVerdict,
    VerificationOutcome,
};

use crate::approvals::ApprovalService;
use crate::binding::GitFactsProvider;
use crate::dispatcher::{ExecError, ExecOutcome, Executor};
use crate::engine::ContextProvider;
use crate::layout::{ExecConfig, Layout};
use crate::pr::{PrClient, approved_body};
use crate::shared::SharedStore;
use crate::static_gates::evaluate as evaluate_static_gates;

/// Largest slice of gate output kept in the gate result artifact.
const OUTPUT_TAIL_CHARS: usize = 4_000;
const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;

fn trim_chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn redacted_transcript(raw: &str) -> String {
    let (clean, _) = turtle_context::redact(raw);
    if clean.len() <= MAX_TRANSCRIPT_BYTES {
        return clean;
    }
    let mut end = MAX_TRANSCRIPT_BYTES;
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[transcript truncated at {} bytes]\n",
        &clean[..end],
        MAX_TRANSCRIPT_BYTES
    )
}

fn redact_gate_output(raw: &str) -> String {
    turtle_context::redact(raw).0
}

pub struct FactoryExecutor<R, C, G> {
    store: SharedStore,
    blobs: Arc<BlobStore>,
    runner: Arc<R>,
    approvals: ApprovalService<C, G>,
    cfg: Arc<ExecConfig>,
}

impl<R, C, G> std::fmt::Debug for FactoryExecutor<R, C, G> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactoryExecutor").finish_non_exhaustive()
    }
}

fn retryable(e: impl std::fmt::Display) -> ExecError {
    ExecError::Retryable(e.to_string())
}

fn fatal(e: impl std::fmt::Display) -> ExecError {
    ExecError::Fatal(e.to_string())
}

fn git_error(e: GitError) -> ExecError {
    match e {
        // Policy refusals are deterministic: retrying cannot help.
        GitError::CommitRefused(_) | GitError::Invalid(_) => fatal(e),
        other => retryable(other),
    }
}

const fn result_ok(end: RunEnd) -> bool {
    matches!(end, RunEnd::Succeeded)
}

/// The snake_case name a domain enum serializes to.
fn label<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn agent_error(e: AgentError) -> ExecError {
    if e.retryable() {
        ExecError::Retryable(e.to_string())
    } else {
        ExecError::Fatal(e.to_string())
    }
}

impl<R, C, G> FactoryExecutor<R, C, G>
where
    R: AgentRunner,
    C: ContextProvider,
    G: GitFactsProvider,
{
    #[must_use]
    pub fn new(
        store: SharedStore,
        blobs: Arc<BlobStore>,
        runner: Arc<R>,
        approvals: ApprovalService<C, G>,
        cfg: Arc<ExecConfig>,
    ) -> Self {
        Self {
            store,
            blobs,
            runner,
            approvals,
            cfg,
        }
    }

    // ------------------------------------------------------------ plumbing

    async fn load(&self, effect: &EffectRow) -> Result<(JobRecord, ProjectRecord), ExecError> {
        let id = effect.job_id.ok_or_else(|| fatal("effect without a job"))?;
        self.store
            .with(move |s| {
                let job = s.load_job(id)?;
                let project = s.load_project(job.project_id)?;
                Ok::<_, turtle_store::StoreError>((job, project))
            })
            .await
            .map_err(retryable)
    }

    fn layout(
        &self,
        project: &ProjectRecord,
        job: &JobRecord,
    ) -> Result<(Layout, String), ExecError> {
        let base = job
            .base_commit
            .clone()
            .ok_or_else(|| fatal("the job has no base commit yet"))?;
        Ok((
            Layout::new(&self.cfg, project, job, &base).map_err(git_error)?,
            base,
        ))
    }

    async fn read_doc<T: DeserializeOwned>(
        &self,
        job: &JobRecord,
        kind: ArtifactKind,
    ) -> Result<Option<(T, RevisionRecord)>, ExecError> {
        let id = job.job.id;
        let Some(rev) = self
            .store
            .with(move |s| s.latest_revision(id, kind))
            .await
            .map_err(retryable)?
        else {
            return Ok(None);
        };
        let bytes = self.blobs.get(&rev.sha256).map_err(fatal)?;
        let doc = serde_json::from_slice(&bytes).map_err(fatal)?;
        Ok(Some((doc, rev)))
    }

    async fn need_doc<T: DeserializeOwned>(
        &self,
        job: &JobRecord,
        kind: ArtifactKind,
        what: &str,
    ) -> Result<(T, RevisionRecord), ExecError> {
        self.read_doc(job, kind)
            .await?
            .ok_or_else(|| fatal(format!("the {what} does not exist yet")))
    }

    async fn store_doc<T: Serialize>(
        &self,
        job: &JobRecord,
        kind: ArtifactKind,
        doc: &T,
        author: Actor,
    ) -> Result<RevisionRecord, ExecError> {
        let sha256 = self.blobs.put_json(doc).map_err(retryable)?;
        let size_bytes = turtle_artifacts::canonical_json(doc).map_err(fatal)?.len() as u64;
        let job_id = job.job.id;
        self.store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.add_revision(&NewRevision {
                        job_id,
                        kind,
                        sha256,
                        size_bytes,
                        media_type: "application/json".into(),
                        schema_id: None,
                        author,
                    })
                })
            })
            .await
            .map_err(retryable)
    }

    /// Runs one agent and records the run as an event. Structured output is parsed by callers.
    /// Runs one agent role with its tokens accounted for: a run row, a reservation against
    /// every budget that applies, the run itself, then settlement with what it really used.
    ///
    /// A hard budget that cannot cover the run holds the job (`BudgetExhausted`) and defers the
    /// effect instead of failing it. A run that fails reports no usage, so its reservation is
    /// released and the run is recorded as `unavailable` rather than guessed at.
    async fn run_agent(
        &self,
        mut req: AgentRequest,
        job: &JobRecord,
    ) -> Result<AgentOutput, ExecError> {
        let context_revision = if let Some(workdir) = req.workdir.as_deref() {
            let areas = req
                .spec
                .as_ref()
                .map_or_else(Vec::new, |spec| spec.affected_areas.clone());
            let discovered: Option<(Value, RevisionRecord)> =
                self.read_doc(job, ArtifactKind::DiscoveryReport).await?;
            let relevant: Vec<String> = discovered
                .and_then(|(report, _)| {
                    report
                        .get("relevant_files")
                        .and_then(Value::as_array)
                        .cloned()
                })
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect();
            // Walking and reading the checkout is blocking file IO; keep it off the async threads.
            let root = workdir.to_path_buf();
            let mut package = tokio::task::spawn_blocking(move || {
                turtle_context::build(
                    &root,
                    &areas,
                    &relevant,
                    turtle_context::DEFAULT_TOKEN_BUDGET,
                )
            })
            .await
            .map_err(retryable)?
            .map_err(fatal)?;
            let terms: Vec<String> = format!("{} {}", job.title, job.description)
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|term| term.len() >= 3)
                .take(12)
                .map(str::to_owned)
                .collect();
            if !terms.is_empty() {
                let query = terms.join(" OR ");
                let project_id = job.project_id;
                let job_id = job.job.id;
                let hits = self
                    .store
                    .with(move |store| store.search_resources(project_id, Some(job_id), &query, 12))
                    .await
                    .map_err(retryable)?;
                for hit in hits {
                    let encoded = serde_json::to_string(&hit.text)
                        .map_err(fatal)?
                        .replace('<', "\\u003c");
                    let title = hit
                        .title
                        .replace('&', "&amp;")
                        .replace('"', "&quot;")
                        .replace('<', "&lt;");
                    let block = format!(
                        "\n\n<resource trust=\"{}\" id=\"{}\" title=\"{}\">\n{}\n</resource>",
                        hit.trust, hit.resource_id, title, encoded
                    );
                    let candidate = format!("{}{block}", package.text);
                    let candidate_tokens = turtle_context::estimate_tokens(&candidate);
                    if candidate_tokens > turtle_context::DEFAULT_TOKEN_BUDGET {
                        continue;
                    }
                    package.text = candidate;
                    let resource_tokens = turtle_context::estimate_tokens(&block);
                    package.manifest.estimated_tokens = candidate_tokens;
                    package
                        .manifest
                        .resources
                        .push(turtle_context::ManifestResource {
                            id: hit.resource_id,
                            title: hit.title,
                            trust: hit.trust,
                            estimated_tokens: resource_tokens,
                            reason: "fts_ranked".into(),
                        });
                }
            }
            package.manifest.content_sha256 = turtle_context::content_sha256(&package.text);
            req.context = Some(AgentContext {
                text: package.text,
                manifest_json: serde_json::to_string(&package.manifest).map_err(fatal)?,
            });
            let bytes = turtle_artifacts::canonical_json(&package.manifest).map_err(fatal)?;
            let hash = self.blobs.put(bytes.as_bytes()).map_err(retryable)?;
            let job_id = job.job.id;
            let size_bytes = bytes.len() as u64;
            Some(
                self.store
                    .with(move |s| {
                        s.transaction(|tx| {
                            tx.add_revision(&NewRevision {
                                job_id,
                                kind: ArtifactKind::ContextManifest,
                                sha256: hash,
                                size_bytes,
                                media_type: "application/json".into(),
                                schema_id: Some("context_manifest.v1".into()),
                                author: Actor::Scheduler,
                            })
                        })
                    })
                    .await
                    .map_err(retryable)?
                    .id,
            )
        } else {
            None
        };
        let (run_id, role) = (req.run_id, req.role);
        let job_id = job.job.id;
        let implementer_provider = if matches!(role, Role::Reviewer | Role::Verifier) {
            self.store
                .with(move |s| s.transaction(|tx| tx.latest_implementer_provider(job_id)))
                .await
                .map_err(retryable)?
        } else {
            None
        };
        let independence = match job.job.profile.review_independence {
            ReviewIndependence::Off => Independence::Off,
            ReviewIndependence::Preferred => Independence::Preferred,
            ReviewIndependence::Required => Independence::Required,
        };
        let (limited, snapshots) = self
            .store
            .with(|s| {
                Ok::<_, turtle_store::StoreError>((
                    s.current_limited_providers()?,
                    s.latest_quota_snapshots()?,
                ))
            })
            .await
            .map_err(retryable)?;
        let limited_providers: Vec<String> = limited.iter().map(|l| l.provider.clone()).collect();
        let quota_threshold = std::env::var("TURTLE_CLAUDE_QUOTA_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| (0.0..=1.0).contains(value))
            .unwrap_or(0.90);
        let avoid_provider = snapshots
            .iter()
            .any(|q| q.provider == "claude-code" && q.utilization >= quota_threshold)
            .then(|| "claude-code".to_owned());
        let route = self
            .runner
            .select(&RouteContext {
                role,
                independence,
                implementer_provider,
                limited_providers,
                avoid_provider,
            })
            .map_err(agent_error)?;
        let provider = route.provider.clone();
        let complexity = req
            .spec
            .as_ref()
            .map_or(turtle_domain::Complexity::XS, |spec| {
                score_complexity(spec, req.plan.as_ref()).class
            });
        let path = BucketPath::new(
            &job.project_id.to_string(),
            role,
            complexity,
            &provider,
            &route.model,
        );
        let lookup_path = path.clone();
        let new_run = NewRun {
            id: run_id,
            job_id,
            role,
            stage: label(&job.job.state),
            provider: provider.clone(),
            model: route.model.clone(),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            context_manifest_rev: context_revision,
        };
        let reserved = self
            .store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.begin_run(&new_run)?;
                    tx.record_routing_decision(&NewRoutingDecision {
                        run: run_id,
                        job: job_id,
                        role,
                        chosen: &format!("{}/{}", route.provider, route.model),
                        candidates: &route.candidates,
                        reason: &route.reason,
                        override_used: route.override_used,
                    })?;
                    let buckets = tx.estimation_buckets(&lookup_path)?;
                    let size = reservation_size(estimate(&buckets, cold_start_estimate(role)));
                    let outcome = tx.reserve_estimated(run_id, job_id, &size)?;
                    if matches!(outcome, ReservationOutcome::Denied { .. }) {
                        tx.cancel_run(run_id, "refused: no budget")?;
                    }
                    Ok((outcome, size.tokens))
                })
            })
            .await
            .map_err(retryable)?;
        let (reserved, reserved_tokens) = reserved;
        if let ReservationOutcome::Denied { budget, requested } = &reserved {
            return Err(ExecError::Hold {
                command: JobCommand::BudgetExhausted,
                reason: format!(
                    "the {} budget has {} tokens left; this run needs about {requested}",
                    budget.scope.name(),
                    budget.headroom()
                ),
            });
        }

        let result = self.runner.run_selected(&req, &provider).await;
        let (project_id, event_usage) = (
            job.project_id,
            result.as_ref().map_or_else(
                |e| e.usage().unwrap_or(UsageValue::Unavailable),
                |o| o.usage,
            ),
        );
        let transcript_raw = match &result {
            Ok(out) => out.transcript.clone().or_else(|| {
                serde_json::to_string(&json!({
                    "type": "result", "result": out.text, "structured_output": out.structured,
                    "usage": out.usage, "permission_denials": out.permission_denials,
                }))
                .ok()
            }),
            Err(e) => e.transcript().map(str::to_owned),
        };
        let transcript = transcript_raw.as_deref().map(redacted_transcript);
        let transcript_blob = transcript
            .as_deref()
            .map(|text| {
                self.blobs
                    .put(text.as_bytes())
                    .map(|hash| (hash, text.len() as u64))
            })
            .transpose()
            .map_err(retryable)?;
        let denials = match &result {
            Ok(out) => out.permission_denials.as_slice(),
            Err(e) => e.permission_denials(),
        };
        let denials: Vec<_> = denials
            .iter()
            .map(|d| {
                let raw =
                    serde_json::to_string(&d.input).unwrap_or_else(|_| "[unavailable]".into());
                let (clean, _) = turtle_context::redact(&raw);
                json!({"tool": d.tool, "input": trim_chars(&clean, 2_000)})
            })
            .collect();
        let (end, error_text, usage) = match &result {
            Ok(out) => (RunEnd::Succeeded, None, out.usage),
            Err(e) => (
                RunEnd::Failed {
                    retryable: e.retryable(),
                },
                Some(e.to_string()),
                e.usage().unwrap_or(UsageValue::Unavailable),
            ),
        };
        let quota = result
            .as_ref()
            .ok()
            .and_then(|out| out.quota.clone())
            .or_else(|| result.as_ref().err().and_then(|err| err.quota().cloned()));
        let rate_limit = result.as_ref().err().and_then(|err| {
            let raw = err
                .raw_message()
                .map(str::to_owned)
                .unwrap_or_else(|| err.to_string());
            err.rate_limit()
                .map(|(provider, reset)| (provider.to_owned(), reset, raw))
        });
        let error_text = error_text.map(|message| turtle_context::redact(&message).0);
        let quota_for_storage = quota.clone();
        let rate_limit_for_storage = rate_limit.clone();
        let provider_for_storage = provider.clone();
        let charge = match usage {
            UsageValue::Reported { usage } | UsageValue::Estimated { usage } => Some(usage.total()),
            // A successful run that reported nothing still used something; assume the estimate.
            UsageValue::Unavailable if result.is_ok() => Some(reserved_tokens),
            UsageValue::Unavailable => None,
        };
        self.store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.finish_run(run_id, end, error_text.as_deref(), usage)?;
                    match charge {
                        Some(tokens) if result_ok(end) => {
                            tx.settle_with_estimation(run_id, tokens, &path)?;
                        }
                        Some(tokens) => {
                            tx.settle(run_id, tokens)?;
                        }
                        None => {
                            tx.release(run_id, "run failed; usage unknown")?;
                        }
                    };
                    if let Some((hash, size)) = transcript_blob {
                        tx.set_transcript_blob(run_id, hash, size)?;
                    }
                    if provider_for_storage == "claude-code"
                        && let Some(QuotaStatus::Partial { windows }) = &quota_for_storage
                    {
                        for window in windows {
                            tx.record_quota_snapshot(
                                "claude-code",
                                run_id,
                                &window.name,
                                window.utilization,
                                window.resets_at,
                                window.status,
                            )?;
                        }
                    }
                    if let Some((limited_provider, reset_at, message)) = &rate_limit_for_storage {
                        tx.observe_provider_limit(limited_provider, run_id, *reset_at, message)?;
                    }
                    for denial in &denials {
                        tx.append_event(&NewEvent {
                            project_id: Some(project_id),
                            job_id: Some(job_id),
                            kind: "run.policy_denied".into(),
                            payload: json!({"run": run_id.to_string(), "denial": denial}),
                            actor: Actor::AgentRun { id: run_id },
                        })?;
                    }
                    if result_ok(end) {
                        tx.append_event(&NewEvent {
                            project_id: Some(project_id),
                            job_id: Some(job_id),
                            kind: "run.completed".into(),
                            payload: json!({
                                "run": run_id.to_string(),
                                "role": role,
                                "provider": provider,
                                "usage": event_usage,
                            }),
                            actor: Actor::AgentRun { id: run_id },
                        })?;
                    }
                    Ok(())
                })
            })
            .await
            .map_err(retryable)?;
        if let Some((limited_provider, reset_at, message)) = rate_limit {
            let latest_limited = self
                .store
                .with(|s| s.current_limited_providers())
                .await
                .map_err(retryable)?;
            let limited_providers = latest_limited
                .iter()
                .map(|limit| limit.provider.clone())
                .collect();
            let route_context = RouteContext {
                role,
                independence,
                implementer_provider: if matches!(role, Role::Reviewer | Role::Verifier) {
                    self.store
                        .with(move |s| s.transaction(|tx| tx.latest_implementer_provider(job_id)))
                        .await
                        .map_err(retryable)?
                } else {
                    None
                },
                limited_providers,
                avoid_provider: None,
            };
            if let Ok(alternative) = self.runner.select(&route_context)
                && alternative.provider != limited_provider
            {
                req.run_id = RunId::new();
                return Box::pin(self.run_agent(req, job)).await;
            }
            let observed_now = self
                .store
                .with(|store| Ok::<_, turtle_store::StoreError>(store.now()))
                .await
                .map_err(retryable)?;
            let due = reset_at.unwrap_or_else(|| observed_now + Duration::from_secs(60 * 60));
            let due_text = turtle_store::clock::format_ts(due).map_err(fatal)?;
            let local_due =
                due.to_offset(turtle_agents::local_offset().unwrap_or(time::UtcOffset::UTC));
            let wait_label = if reset_at.is_some() {
                format!(
                    " until {}",
                    local_due
                        .format(time::macros::format_description!("[hour]:[minute]"))
                        .unwrap_or_else(|_| "unknown time".into())
                )
            } else {
                "; reset unknown; retrying conservatively in one hour".into()
            };
            let reason = format!("{limited_provider} is rate limited{wait_label}: {message}");
            return Err(ExecError::HoldUntil {
                command: JobCommand::RateLimited {
                    provider: limited_provider,
                },
                reason,
                not_before: due_text,
            });
        }
        result.map_err(agent_error)
    }

    /// Probe for agent-produced documents: a revision this very effect already stored (created
    /// after the effect, by the scheduler, not edited by a user) means an earlier attempt got as
    /// far as storing its result before a crash. Reusing it avoids paying for the agent twice.
    async fn reuse<T: DeserializeOwned>(
        &self,
        job: &JobRecord,
        kind: ArtifactKind,
        effect: &EffectRow,
    ) -> Result<Option<T>, ExecError> {
        let Some((doc, rev)) = self.read_doc::<T>(job, kind).await? else {
            return Ok(None);
        };
        let ours = rev.author_kind == "system" && rev.id > effect.id.to_string();
        Ok(ours.then_some(doc))
    }

    fn request(&self, job: &JobRecord, role: Role) -> AgentRequest {
        AgentRequest {
            run_id: RunId::new(),
            role,
            job_key: job.key.to_string(),
            title: job.title.clone(),
            description: job.description.clone(),
            workdir: None,
            spec: None,
            plan: None,
            task: None,
            feedback: None,
            diff_summary: None,
            context: None,
        }
    }

    /// A request for a role that runs before the job has a worktree (discovery, specification,
    /// planning). Such roles are read-only, so they read the project checkout itself.
    fn request_in_checkout(
        &self,
        job: &JobRecord,
        project: &ProjectRecord,
        role: Role,
    ) -> AgentRequest {
        let mut req = self.request(job, role);
        req.workdir = Some(std::path::PathBuf::from(&project.repo_path));
        req
    }

    fn parse<T: DeserializeOwned>(out: &AgentOutput, what: &str) -> Result<T, ExecError> {
        let value = out
            .structured
            .clone()
            .ok_or_else(|| retryable(format!("the agent returned no {what}")))?;
        serde_json::from_value(value).map_err(|e| retryable(format!("invalid {what}: {e}")))
    }

    /// Runs a code-touching agent in the worktree under the Git policy: snapshot first, check
    /// afterwards, and undo (preserving everything) if the agent committed, switched branches,
    /// moved refs or touched Git configuration.
    async fn run_code_agent(
        &self,
        req: AgentRequest,
        job: &JobRecord,
        wt: &Repo,
    ) -> Result<AgentOutput, ExecError> {
        let before = wt.snapshot().await.map_err(git_error)?;
        let out = self.run_agent(req, job).await;
        let check = wt.check_policy(&before).await.map_err(git_error)?;
        if !check.is_ok() {
            let run = RunId::new().to_string();
            wt.quarantine(&run, &before).await.map_err(git_error)?;
            return Err(fatal(format!(
                "the agent violated the Git policy: {}. Its work was preserved at refs/turtle/recovery/{run}",
                check.violations().join("; ")
            )));
        }
        out
    }

    async fn trailers(
        &self,
        job: &JobRecord,
        task: Option<String>,
        run: RunId,
    ) -> Result<Trailers, ExecError> {
        let run_key = run.to_string();
        let job_id = job.job.id;
        let agent_commit = task.is_some();
        let provider = self
            .store
            .with(move |s| {
                if agent_commit {
                    s.run_by_id(&run_key).map(|r| Some(r.provider))
                } else {
                    s.transaction(|tx| tx.latest_implementer_provider(job_id))
                }
            })
            .await
            .map_err(retryable)?;
        Ok(Trailers {
            job: job.key.to_string(),
            task,
            run: Some(run.to_string()),
            provider: Some(provider.unwrap_or_else(|| self.runner.name().to_owned())),
            model: None,
            context: None,
        })
    }

    // ------------------------------------------------------------ effects

    async fn start_run(&self, effect: &EffectRow, role: Role) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        match role {
            Role::Discovery => {
                let req = self.request_in_checkout(&job, &project, role);
                let out = self.run_agent(req, &job).await?;
                let doc = out
                    .structured
                    .unwrap_or_else(|| json!({"summary": out.text}));
                self.store_doc(&job, ArtifactKind::DiscoveryReport, &doc, Actor::Scheduler)
                    .await?;
                Ok(ExecOutcome::done(json!({"role": "discovery"}))
                    .with_command(JobCommand::DiscoveryCompleted { report_valid: true }))
            }
            Role::SpecWriter => {
                let spec: Specification = match self
                    .reuse(&job, ArtifactKind::Specification, effect)
                    .await?
                {
                    Some(spec) => spec,
                    None => {
                        let mut req = self.request_in_checkout(&job, &project, role);
                        req.feedback = self.last_human_feedback(&job).await;
                        let out = self.run_agent(req, &job).await?;
                        let spec: Specification = Self::parse(&out, "specification")?;
                        spec.validate()
                            .map_err(|e| retryable(format!("invalid specification: {e}")))?;
                        self.store_doc(&job, ArtifactKind::Specification, &spec, Actor::Scheduler)
                            .await?;
                        spec
                    }
                };
                Ok(
                    ExecOutcome::done(json!({"criteria": spec.criteria_count()})).with_command(
                        JobCommand::SpecProduced {
                            valid: true,
                            criteria_count: spec.criteria_count(),
                        },
                    ),
                )
            }
            Role::Planner => {
                let (spec, _): (Specification, _) = self
                    .need_doc(&job, ArtifactKind::Specification, "specification")
                    .await?;
                let plan: Plan = match self.reuse(&job, ArtifactKind::Plan, effect).await? {
                    Some(plan) => plan,
                    None => {
                        let mut req = self.request_in_checkout(&job, &project, role);
                        req.spec = Some(spec.clone());
                        req.feedback = self.last_human_feedback(&job).await;
                        let out = self.run_agent(req, &job).await?;
                        let plan: Plan = Self::parse(&out, "plan")?;
                        plan.validate(&spec)
                            .map_err(|e| retryable(format!("invalid plan: {e}")))?;
                        self.store_doc(&job, ArtifactKind::Plan, &plan, Actor::Scheduler)
                            .await?;
                        plan
                    }
                };
                let within_scope = plan
                    .tasks
                    .iter()
                    .flat_map(|t| &t.expected_paths)
                    .all(|p| within_areas(p, &spec.affected_areas));
                Ok(ExecOutcome::done(
                    json!({"tasks": plan.task_count(), "within_scope": within_scope}),
                )
                .with_command(JobCommand::PlanProduced {
                    valid: true,
                    within_scope,
                }))
            }
            Role::Reviewer => {
                let report: ReviewReport = match self
                    .reuse(&job, ArtifactKind::ReviewReport, effect)
                    .await?
                {
                    Some(report) => report,
                    None => {
                        let (spec, _): (Specification, _) = self
                            .need_doc(&job, ArtifactKind::Specification, "specification")
                            .await?;
                        let (layout, base) = self.layout(&project, &job)?;
                        let repo = self.cfg.repo(&project);
                        let head = repo
                            .branch_tip(&layout.spec.branch)
                            .await
                            .map_err(git_error)?
                            .ok_or_else(|| fatal("the job branch does not exist"))?;
                        let mut req = self.request(&job, role);
                        req.spec = Some(spec);
                        req.workdir = Some(layout.path.clone());
                        req.diff_summary = Some(self.diff_summary(&repo, &base, &head).await?);
                        let out = self.run_agent(req, &job).await?;
                        let report: ReviewReport = Self::parse(&out, "review report")?;
                        self.store_doc(&job, ArtifactKind::ReviewReport, &report, Actor::Scheduler)
                            .await?;
                        report
                    }
                };
                let (min_fix, max_auto_fix) = match job.job.profile.base {
                    ProfileBase::Fast => (Severity::Major, Severity::Major),
                    ProfileBase::Standard => (Severity::Major, Severity::Major),
                    ProfileBase::Strict => (Severity::Nit, Severity::Nit),
                };
                if !report.scope_creep
                    && !report.replan
                    && report.findings.iter().any(|f| f.severity > max_auto_fix)
                {
                    return Ok(ExecOutcome::done(json!({"findings": report.findings.len(),
                        "decision": "human_review_required"}))
                    .with_command(JobCommand::Halt {
                        reason: BlockReason::ReviewDecisionRequired,
                    }));
                }
                let verdict = if report.scope_creep {
                    ReviewVerdict::ScopeCreep
                } else if report.replan {
                    ReviewVerdict::Replan
                } else if report.findings.iter().any(|f| f.severity >= min_fix) {
                    ReviewVerdict::Fixable
                } else {
                    ReviewVerdict::Pass
                };
                Ok(
                    ExecOutcome::done(json!({"findings": report.findings.len()}))
                        .with_command(JobCommand::ReviewCompleted { verdict }),
                )
            }
            Role::Fixer => self.fix(&job, &project).await,
            _ => Ok(ExecOutcome::done(
                json!({"note": "role has no executor yet"}),
            )),
        }
    }

    /// The most recent thing a human asked for in an approval comment, if any.
    async fn last_human_feedback(&self, job: &JobRecord) -> Option<String> {
        let id = job.job.id;
        let approvals = self.store.with(move |s| s.list_approvals(id)).await.ok()?;
        approvals
            .into_iter()
            .rev()
            .find(|a| a.status == ApprovalStatus::ChangesRequested && a.comment.is_some())
            .and_then(|a| a.comment)
    }

    async fn diff_summary(&self, repo: &Repo, base: &str, head: &str) -> Result<String, ExecError> {
        let files = repo.changed_files(base, head).await.map_err(git_error)?;
        Ok(files
            .iter()
            .map(|f| {
                format!(
                    "{} {} (+{} -{})",
                    f.status,
                    f.path,
                    f.added.map_or("?".into(), |n| n.to_string()),
                    f.removed.map_or("?".into(), |n| n.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    /// What a fixer must address: the newest of failed gates, blocking review findings, unmet
    /// criteria and human comments.
    async fn fix_feedback(&self, job: &JobRecord) -> String {
        let mut candidates: Vec<(String, String)> = Vec::new();
        if let Ok(Some((gate, rev))) = self.read_doc::<Value>(job, ArtifactKind::GateResult).await
            && gate["passed"] == json!(false)
        {
            candidates.push((rev.id, format!("Failed checks:\n{gate}")));
        }
        if let Ok(Some((review, rev))) = self
            .read_doc::<ReviewReport>(job, ArtifactKind::ReviewReport)
            .await
        {
            let blocking = review.blocking_findings();
            if !blocking.is_empty() {
                let text = blocking
                    .iter()
                    .map(|f| format!("[{:?}] {}", f.severity, f.message))
                    .collect::<Vec<_>>()
                    .join("\n");
                candidates.push((rev.id, format!("Review findings to fix:\n{text}")));
            }
        }
        if let Ok(Some((ver, rev))) = self
            .read_doc::<VerificationReport>(job, ArtifactKind::VerificationReport)
            .await
        {
            let unmet: Vec<_> = ver
                .criteria
                .iter()
                .filter(|c| c.status != turtle_domain::CriterionStatus::Met)
                .map(|c| format!("{}: {}", c.id, c.evidence))
                .collect();
            if !unmet.is_empty() {
                candidates.push((rev.id, format!("Unmet criteria:\n{}", unmet.join("\n"))));
            }
        }
        // ULIDs sort by creation time, so the largest id is the newest.
        candidates.sort();
        candidates
            .pop()
            .map_or_else(|| "Address the open issues.".to_owned(), |(_, text)| text)
    }

    async fn fix(
        &self,
        job: &JobRecord,
        project: &ProjectRecord,
    ) -> Result<ExecOutcome, ExecError> {
        let (layout, _base) = self.layout(project, job)?;
        let wt = self.cfg.repo(project).at(&layout.path);
        let plan: Option<(Plan, _)> = self.read_doc(job, ArtifactKind::Plan).await?;
        let spec: Option<(Specification, _)> =
            self.read_doc(job, ArtifactKind::Specification).await?;
        let mut req = self.request(job, Role::Fixer);
        req.workdir = Some(layout.path.clone());
        req.plan = plan.map(|p| p.0);
        req.spec = spec.map(|s| s.0);
        req.feedback = Some(self.fix_feedback(job).await);
        let run_id = RunId::new();
        req.run_id = run_id;
        self.run_code_agent(req, job, &wt).await?;
        let key = job.key.to_string();
        wt.commit_all(
            &format!("fix({key}): address open issues"),
            "",
            &self
                .trailers(job, Some(format!("fix/{run_id}")), run_id)
                .await?,
        )
        .await
        .map_err(git_error)?;
        Ok(ExecOutcome::done(json!({"fixed": true})).with_command(JobCommand::AllTasksSucceeded))
    }

    async fn create_worktree(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let repo = self.cfg.repo(&project);
        let target = project
            .settings
            .target_branch
            .clone()
            .unwrap_or_else(|| project.default_branch.clone());

        // The base commit is decided once and recorded before any Git change, so a retry after
        // a crash recreates exactly the same worktree.
        let base = match job.base_commit.clone() {
            Some(b) => b,
            None => {
                let tip = repo
                    .branch_tip(&target)
                    .await
                    .map_err(git_error)?
                    .ok_or_else(|| fatal(format!("target branch `{target}` does not exist")))?;
                let (id, c) = (job.job.id, tip.clone());
                self.store
                    .with(move |s| s.transaction(|tx| tx.set_base_commit(id, &c)))
                    .await
                    .map_err(retryable)?;
                tip
            }
        };
        let layout = Layout::new(&self.cfg, &project, &job, &base).map_err(git_error)?;
        let outcome = repo
            .ensure_worktree(&layout.spec)
            .await
            .map_err(git_error)?;

        // Setup (installing dependencies…) is marked done inside the Git directory, so a crash
        // between "worktree created" and "setup finished" re-runs setup, and nothing else does.
        let wt = repo.at(&layout.path);
        let marker = wt
            .git_dir()
            .await
            .map_err(git_error)?
            .join("turtle-setup-done");
        if let Some(setup) = project.settings.commands.worktree_setup.as_deref()
            && !marker.exists()
        {
            let result = self.run_shell(&project, &layout.path, setup).await?;
            if result.outcome != "passed" {
                return Err(fatal(format!("worktree setup failed: {}", result.tail)));
            }
        }
        std::fs::write(&marker, b"").map_err(retryable)?;
        Ok(ExecOutcome::done(json!({
            "path": layout.path,
            "branch": layout.spec.branch,
            "base": base,
            "created": outcome == WorktreeOutcome::Created,
        })))
    }

    async fn dispatch_ready_tasks(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let (layout, base) = self.layout(&project, &job)?;
        let (spec, _): (Specification, _) = self
            .need_doc(&job, ArtifactKind::Specification, "specification")
            .await?;
        let (plan, plan_rev): (Plan, RevisionRecord) =
            self.need_doc(&job, ArtifactKind::Plan, "plan").await?;
        let repo = self.cfg.repo(&project);
        let wt = repo.at(&layout.path);
        let key = job.key.to_string();

        let mut done = 0;
        for task in plan.execution_order().map_err(fatal)? {
            let task_key = format!("{}/{}", plan_rev.id, task.id);
            // Probe: a commit with this task's trailer means it already finished.
            if repo
                .find_commit_by_task(&layout.spec.branch, &task_key)
                .await
                .map_err(git_error)?
                .is_some()
            {
                done += 1;
                continue;
            }
            let run_id = RunId::new();
            let mut req = self.request(&job, Role::Implementer);
            req.run_id = run_id;
            req.workdir = Some(layout.path.clone());
            req.spec = Some(spec.clone());
            req.plan = Some(plan.clone());
            req.task = Some(task.clone());
            self.run_code_agent(req, &job, &wt).await?;
            wt.commit_all(
                &format!("feat({key}): {}", task.title),
                &task.instructions,
                &self.trailers(&job, Some(task_key), run_id).await?,
            )
            .await
            .map_err(git_error)?;
            done += 1;
        }

        let head = wt.head().await.map_err(git_error)?;
        if head == base {
            return Err(fatal("the implementation produced no changes at all"));
        }
        Ok(ExecOutcome::done(json!({"tasks": done, "head": head}))
            .with_command(JobCommand::AllTasksSucceeded))
    }

    async fn run_shell(
        &self,
        project: &ProjectRecord,
        dir: &Path,
        command: &str,
    ) -> Result<GateRun, ExecError> {
        let timeout = project
            .settings
            .gate_timeout_secs
            .map_or(self.cfg.default_gate_timeout, Duration::from_secs);
        let mut spec = CommandSpec::new("sh")
            .args(["-c", command])
            .cwd(dir)
            .timeout(timeout)
            .max_output_bytes(1 << 20);
        spec.inherit_env = project.settings.env_allowlist.clone();
        let out = turtle_exec::run(spec).await.map_err(retryable)?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        text = redact_gate_output(&text);
        let tail: String = {
            let chars: Vec<char> = text.chars().collect();
            chars[chars.len().saturating_sub(OUTPUT_TAIL_CHARS)..]
                .iter()
                .collect()
        };
        let (outcome, exit_code) = match out.outcome {
            ExitOutcome::Exited(0) => ("passed", Some(0)),
            ExitOutcome::Exited(c) => ("failed", Some(c)),
            ExitOutcome::Signaled => ("failed", None),
            ExitOutcome::TimedOut => ("timeout", None),
        };
        Ok(GateRun {
            outcome,
            exit_code,
            duration_ms: u64::try_from(out.duration.as_millis()).unwrap_or(u64::MAX),
            tail,
        })
    }

    async fn run_gates(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let (layout, _) = self.layout(&project, &job)?;
        let wt = self.cfg.repo(&project).at(&layout.path);
        let head = wt.head().await.map_err(git_error)?;
        let tree = wt.tree_of("HEAD").await.map_err(git_error)?;

        let (spec, _): (Specification, _) = self
            .need_doc(&job, ArtifactKind::Specification, "specification")
            .await?;
        let changes = wt
            .changed_files(job.base_commit.as_deref().unwrap_or(&head), &head)
            .await
            .map_err(git_error)?;
        let diff = wt
            .diff_bytes(job.base_commit.as_deref().unwrap_or(&head), &head)
            .await
            .map_err(git_error)?;
        let static_report = evaluate_static_gates(
            &project.settings.static_gates,
            project.settings.protected_paths.as_deref(),
            &spec,
            &changes,
            &diff,
        );
        let mut results = static_report.results;
        let mut passed = static_report.passed;
        for (name, command) in project.settings.commands.gates() {
            let run = self.run_shell(&project, &layout.path, command).await?;
            passed &= run.outcome == "passed";
            results.push(json!({
                "gate": name, "command": command, "outcome": run.outcome,
                "exit_code": run.exit_code, "duration_ms": run.duration_ms,
                "output_tail": run.tail,
            }));
        }
        let doc = json!({
            "head_commit": head, "tree_hash": tree, "passed": passed,
            "configured": !results.is_empty(), "results": results, "acknowledgement_flags": static_report.acknowledgement_flags,
        });
        self.store_doc(&job, ArtifactKind::GateResult, &doc, Actor::Scheduler)
            .await?;
        Ok(
            ExecOutcome::done(json!({"passed": passed})).with_command(JobCommand::GatesFinished {
                all_required_passed: passed,
            }),
        )
    }

    async fn run_verification(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let (spec, _): (Specification, _) = self
            .need_doc(&job, ArtifactKind::Specification, "specification")
            .await?;
        let report: VerificationReport = match self
            .reuse(&job, ArtifactKind::VerificationReport, effect)
            .await?
        {
            Some(report) => report,
            None => {
                let (layout, base) = self.layout(&project, &job)?;
                let repo = self.cfg.repo(&project);
                let head = repo
                    .branch_tip(&layout.spec.branch)
                    .await
                    .map_err(git_error)?
                    .ok_or_else(|| fatal("the job branch does not exist"))?;
                let mut req = self.request(&job, Role::Verifier);
                req.spec = Some(spec.clone());
                req.workdir = Some(layout.path.clone());
                req.diff_summary = Some(self.diff_summary(&repo, &base, &head).await?);
                let out = self.run_agent(req, &job).await?;
                let report: VerificationReport = Self::parse(&out, "verification report")?;
                self.store_doc(
                    &job,
                    ArtifactKind::VerificationReport,
                    &report,
                    Actor::Scheduler,
                )
                .await?;
                report
            }
        };
        let outcome = match report.decision(&spec) {
            VerificationDecision::Met => VerificationOutcome::Met,
            VerificationDecision::NotMetFixable => VerificationOutcome::NotMetFixable,
            VerificationDecision::NotMetStructural => VerificationOutcome::NotMetStructural,
        };
        Ok(
            ExecOutcome::done(json!({"decision": report.decision(&spec)}))
                .with_command(JobCommand::VerificationCompleted { outcome }),
        )
    }

    async fn request_approval(
        &self,
        effect: &EffectRow,
        gate: GateKind,
    ) -> Result<ExecOutcome, ExecError> {
        let id = effect.job_id.ok_or_else(|| fatal("effect without a job"))?;
        if gate == GateKind::BudgetIncrease {
            // A held job is resumed by raising its budget (`turtle budget set`), not by an
            // approval row that nothing could decide yet.
            return Ok(ExecOutcome::done(
                json!({"note": "raise the budget to continue"}),
            ));
        }
        let (approval, created) = self.approvals.request(id, gate).await.map_err(retryable)?;
        Ok(ExecOutcome::done(json!({
            "approval": approval.id.to_string(),
            "subject_hash": approval.subject_hash.to_string(),
            "created": created,
        })))
    }

    async fn deliver(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let (layout, base) = self.layout(&project, &job)?;
        let repo = self.cfg.repo(&project);
        let key = job.key.to_string();
        let target = layout.target_branch.clone();
        let id = job.job.id;
        if project.settings.delivery == DeliveryMode::Pr
            && let Some(existing) = self
                .store
                .with(move |s| s.delivery(id))
                .await
                .map_err(retryable)?
            && existing.mode == "pr"
            && existing.status == "pending"
        {
            return Ok(ExecOutcome::done(
                json!({"pr_url": existing.pr_url, "status": "pending"}),
            ));
        }
        if project.settings.delivery == DeliveryMode::LocalSquash {
            let parked = format!("refs/turtle/delivery/{key}");
            let recovered = if let Some(commit) = repo
                .find_delivery_commit(&target, &key)
                .await
                .map_err(git_error)?
            {
                Some(DeliveryOutcome::Delivered { commit })
            } else if let Ok(commit) = repo.rev(&parked).await {
                Some(DeliveryOutcome::PendingIntegration {
                    commit,
                    reference: parked,
                })
            } else {
                None
            };
            if let Some(outcome) = recovered {
                let approval = self
                    .approvals
                    .current_approved(&job, GateKind::Acceptance)
                    .await
                    .map_err(fatal)?;
                let (commit, status) = match &outcome {
                    DeliveryOutcome::Delivered { commit } => {
                        (commit.clone(), DeliveryStatus::Delivered)
                    }
                    DeliveryOutcome::PendingIntegration { commit, .. } => {
                        (commit.clone(), DeliveryStatus::PendingIntegration)
                    }
                };
                let (c, t, aid) = (commit.clone(), target.clone(), approval.id);
                self.store
                    .with(move |s| {
                        s.transaction(|tx| {
                            tx.record_delivery(id, "local_squash", &t, &c, status, aid)
                        })
                    })
                    .await
                    .map_err(retryable)?;
                return Ok(
                    ExecOutcome::done(json!({"commit": commit, "status": "recovered"}))
                        .with_command(JobCommand::Delivered),
                );
            }
        }
        let tip = repo
            .branch_tip(&target)
            .await
            .map_err(git_error)?
            .ok_or_else(|| fatal(format!("target branch `{target}` does not exist")))?;
        if tip != base {
            let wt = repo.at(&layout.path);
            let head = wt.head().await.map_err(git_error)?;
            if !repo.is_ancestor(&tip, &head).await.map_err(git_error)? {
                self.approvals
                    .current_approved(&job, GateKind::Acceptance)
                    .await
                    .map_err(fatal)?;
            }
            match repo
                .refresh_job(&wt, &base, &tip, &key)
                .await
                .map_err(git_error)?
            {
                RefreshOutcome::Conflicts { files } => {
                    return Ok(ExecOutcome::done(json!({"conflicts": files,
                        "resolution": format!("resolve the conflicts on {}, incorporate {}, then run `turtle job resolve {key}`", layout.spec.branch, target)}))
                        .with_command(JobCommand::ConflictDetected));
                }
                RefreshOutcome::Refreshed { head } => {
                    let old = base.clone();
                    let new = tip.clone();
                    self.store
                        .with(move |s| s.transaction(|tx| tx.refresh_base_commit(id, &old, &new)))
                        .await
                        .map_err(retryable)?;
                    return Ok(ExecOutcome::done(json!({"base": tip, "head": head}))
                        .with_command(JobCommand::BaseRefreshed));
                }
            }
        }
        let approved = self
            .store
            .with(move |s| s.latest_approved(id, GateKind::Acceptance))
            .await
            .map_err(retryable)?
            .ok_or_else(|| fatal("delivery needs an approved acceptance"))?;
        if approved
            .binding
            .git
            .as_ref()
            .is_some_and(|g| g.base_commit != base)
        {
            // A crash may have occurred after persisting a refreshed base but before the
            // BaseRefreshed command invalidated acceptance and scheduled fresh gates.
            return Ok(ExecOutcome::done(json!({"base": base, "recovered": true}))
                .with_command(JobCommand::BaseRefreshed));
        }
        let approval = self
            .approvals
            .current_approved(&job, GateKind::Acceptance)
            .await
            .map_err(fatal)?;

        if project.settings.delivery == DeliveryMode::Pr {
            return self.deliver_pr(&job, &project, &layout, &approval).await;
        }

        // Probes: an earlier attempt may already have delivered or parked the result.
        let parked = format!("refs/turtle/delivery/{key}");
        let outcome = if let Some(commit) = repo
            .find_delivery_commit(&target, &key)
            .await
            .map_err(git_error)?
        {
            DeliveryOutcome::Delivered { commit }
        } else if let Ok(commit) = repo.rev(&parked).await {
            DeliveryOutcome::PendingIntegration {
                commit,
                reference: parked,
            }
        } else {
            let (spec, _): (Specification, _) = self
                .need_doc(&job, ArtifactKind::Specification, "specification")
                .await?;
            let trailers = self.trailers(&job, None, RunId::new()).await?.render();
            let req = SquashRequest {
                target_branch: &target,
                job_branch: &layout.spec.branch,
                job_key: &key,
                subject: &format!("feat({key}): {}", job.title),
                body: &spec.summary,
                trailers: &trailers,
            };
            match repo.deliver_squash_from(&req, &tip).await {
                Ok(o) => o,
                Err(GitError::Conflicts(files)) => {
                    return Ok(ExecOutcome::done(json!({"conflicts": files}))
                        .with_command(JobCommand::ConflictDetected));
                }
                // The target moved while we worked: try again on the new tip.
                Err(e @ GitError::TargetMoved { .. }) => return Err(retryable(e)),
                Err(e) => return Err(git_error(e)),
            }
        };

        let (commit, status) = match &outcome {
            DeliveryOutcome::Delivered { commit } => (commit.clone(), DeliveryStatus::Delivered),
            DeliveryOutcome::PendingIntegration { commit, .. } => {
                (commit.clone(), DeliveryStatus::PendingIntegration)
            }
        };
        let (c, t, aid) = (commit.clone(), target.clone(), approval.id);
        self.store
            .with(move |s| {
                s.transaction(|tx| tx.record_delivery(id, "local_squash", &t, &c, status, aid))
            })
            .await
            .map_err(retryable)?;
        let note = match &outcome {
            DeliveryOutcome::Delivered { .. } => "delivered".to_owned(),
            DeliveryOutcome::PendingIntegration { reference, .. } => format!(
                "pending integration: `{target}` is checked out, so it was not moved. Run `git merge --ff-only {reference}`"
            ),
        };
        Ok(ExecOutcome::done(json!({"commit": commit, "status": note}))
            .with_command(JobCommand::Delivered))
    }

    async fn deliver_pr(
        &self,
        job: &JobRecord,
        project: &ProjectRecord,
        layout: &Layout,
        approval: &turtle_store::ApprovalRecord,
    ) -> Result<ExecOutcome, ExecError> {
        let id = job.job.id;
        let repo = self.cfg.repo(project);
        let head = repo
            .branch_tip(&layout.spec.branch)
            .await
            .map_err(git_error)?
            .ok_or_else(|| fatal("job branch is missing"))?;
        let spec_ref = approval
            .binding
            .artifacts
            .get("specification")
            .ok_or_else(|| fatal("acceptance approval has no specification"))?;
        let bytes = self.blobs.get(&spec_ref.sha256).map_err(fatal)?;
        let spec: Specification = serde_json::from_slice(&bytes).map_err(fatal)?;
        let scope = self
            .approvals
            .current_approved(job, GateKind::Scope)
            .await
            .map_err(fatal)?;
        let plan = self
            .store
            .with(move |s| s.latest_approved(id, GateKind::Plan))
            .await
            .map_err(retryable)?;
        let plan_hash = if plan.is_some() {
            Some(
                self.approvals
                    .current_approved(job, GateKind::Plan)
                    .await
                    .map_err(fatal)?
                    .subject_hash
                    .to_hex(),
            )
        } else {
            None
        };
        if job.job.profile.plan_approval_required(false) && plan_hash.is_none() {
            return Err(fatal(
                "this profile requires an approved plan before PR delivery",
            ));
        }
        let body = approved_body(
            &spec,
            &scope.subject_hash.to_hex(),
            plan_hash.as_deref(),
            &approval.subject_hash.to_hex(),
        );
        let client = PrClient::new(self.cfg.gh_program.clone());
        let existing = client
            .find(Path::new(&project.repo_path), &layout.spec.branch)
            .await
            .map_err(retryable)?;
        let url = if let Some(pr) = existing {
            if pr.head_ref_oid.as_deref() != Some(head.as_str()) {
                return Err(fatal(
                    "the existing PR has a different head; inspect it before delivery",
                ));
            }
            if pr.body.as_deref() != Some(body.as_str()) || pr.state == "CLOSED" {
                return Err(fatal(
                    "the existing PR body or state differs from the approved delivery",
                ));
            }
            pr.url
        } else {
            // The acceptance check above precedes this first remote mutation.
            repo.run(&[
                "push",
                "origin",
                &format!("refs/heads/{0}:refs/heads/{0}", layout.spec.branch),
            ])
            .await
            .map_err(git_error)?;
            client
                .create(
                    Path::new(&project.repo_path),
                    &layout.target_branch,
                    &layout.spec.branch,
                    &format!("{}: {}", job.key, job.title),
                    &body,
                )
                .await
                .map_err(retryable)?
        };
        let target = layout.target_branch.clone();
        let a = approval.id;
        let h = head.clone();
        let u = url.clone();
        self.store
            .with(move |s| s.transaction(|tx| tx.record_pr(id, &target, &h, &u, a)))
            .await
            .map_err(retryable)?;
        Ok(ExecOutcome::done(
            json!({"pr_url": url, "status": "pending"}),
        ))
    }

    async fn cleanup_worktree(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let (job, project) = self.load(effect).await?;
        let Ok((layout, _)) = self.layout(&project, &job) else {
            return Ok(ExecOutcome::done(
                json!({"note": "no worktree was ever created"}),
            ));
        };
        self.cfg
            .repo(&project)
            .remove_worktree(&layout.path, true)
            .await
            .map_err(git_error)?;
        Ok(ExecOutcome::done(json!({"removed": layout.path})))
    }
}

struct GateRun {
    outcome: &'static str,
    exit_code: Option<i32>,
    duration_ms: u64,
    tail: String,
}

impl<R, C, G> Executor for FactoryExecutor<R, C, G>
where
    R: AgentRunner,
    C: ContextProvider,
    G: GitFactsProvider,
{
    async fn execute(&self, effect: &EffectRow) -> Result<ExecOutcome, ExecError> {
        let parsed: Effect = serde_json::from_value(effect.payload.clone())
            .map_err(|e| fatal(format!("unreadable effect payload: {e}")))?;
        match parsed {
            Effect::StartRun { role, .. } => self.start_run(effect, role).await,
            Effect::RequestApproval { gate, .. } => self.request_approval(effect, gate).await,
            Effect::CreateWorktree => self.create_worktree(effect).await,
            Effect::DispatchReadyTasks => self.dispatch_ready_tasks(effect).await,
            Effect::RunGates => self.run_gates(effect).await,
            Effect::RunVerification => self.run_verification(effect).await,
            Effect::Deliver => self.deliver(effect).await,
            Effect::CleanupWorktree => self.cleanup_worktree(effect).await,
            Effect::SupersedePendingApprovals => {
                let id = effect.job_id.ok_or_else(|| fatal("effect without a job"))?;
                let n = self
                    .store
                    .with(move |s| s.transaction(|tx| tx.supersede_pending_approvals(id)))
                    .await
                    .map_err(retryable)?;
                Ok(ExecOutcome::done(json!({"superseded": n})))
            }
            // No live runs are tracked yet (no run supervisor); nothing to terminate or release.
            Effect::TerminateRuns | Effect::ReleaseReservations | Effect::NotifyUser => {
                Ok(ExecOutcome::done(json!({"note": "nothing to do yet"})))
            }
        }
    }
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    #[test]
    fn transcript_and_gate_redaction_remove_corpus_secrets() {
        let secret = "api_key = ghp_123456789012345678901234567890123456";
        let stored = redacted_transcript(&format!("{{\"message\":\"{secret}\"}}\n"));
        assert!(!stored.contains("ghp_123456789012345678901234567890123456"));
        assert!(stored.contains("[REDACTED:"));
        let gate = redact_gate_output(&format!("test output: {secret}"));
        assert!(!gate.contains("ghp_123456789012345678901234567890123456"));
    }
}
