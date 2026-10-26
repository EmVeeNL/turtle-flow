//! `turtle job create | list | show | events | watch | start | pause | resume | worktree | revert | cancel`.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use turtle_domain::{Actor, ArtifactKind, JobKey, JobState, JobType, Priority};
use turtle_scheduler::{Layout, pr::PrClient};
use turtle_store::{BudgetScope, JobRecord, NewJob, ProjectRecord};
use turtle_workflow::{Hold, JobCommand, PauseMode, Profile, ProfileBase};

use crate::runtime::{Runtime, label, parse_label};

#[derive(Debug, Subcommand)]
pub enum JobCommandLine {
    /// Create a job in the backlog.
    Create {
        /// What to build, in one line.
        title: String,
        /// Project slug (optional when only one project is registered).
        #[arg(short, long)]
        project: Option<String>,
        /// Longer description; use --file to read it from a Markdown file instead.
        #[arg(short, long, conflicts_with = "file")]
        description: Option<String>,
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,
        /// feature, bugfix, refactor, docs or chore.
        #[arg(long = "type", default_value = "feature")]
        job_type: String,
        /// low, normal, high or urgent.
        #[arg(long, default_value = "normal")]
        priority: String,
        /// fast, standard or strict (default: the project's profile).
        #[arg(long, value_parser = ["fast", "standard", "strict"])]
        profile: Option<String>,
        /// Token budget for this job, e.g. `400k`; the job waits for you when it is used up.
        #[arg(long)]
        budget: Option<String>,
        /// Start the job right away.
        #[arg(long)]
        start: bool,
    },
    /// List jobs, newest first.
    List {
        #[arg(short, long)]
        project: Option<String>,
        /// Only jobs that are not finished.
        #[arg(long)]
        active: bool,
    },
    /// Show one job.
    Show {
        key: String,
    },
    /// Print the job's event history.
    Events {
        key: String,
        #[arg(long)]
        json: bool,
    },
    /// Follow a job until it finishes or needs you.
    Watch {
        key: String,
    },
    /// Start a job from the backlog.
    Start {
        key: String,
    },
    /// Pause a job.
    Pause {
        key: String,
        /// Stop the current agent run too (not implemented yet: the run finishes first).
        #[arg(long)]
        hard: bool,
    },
    Resume {
        key: String,
    },
    /// Continue a blocked conflict or authorize a fix after review findings.
    Resolve {
        key: String,
        /// For review findings: implementation, planning or specification.
        #[arg(long)]
        target: Option<String>,
    },
    /// Check whether a pending pull request has merged and finish delivery.
    Sync {
        key: String,
    },
    /// Print the deterministic worktree path for a job.
    Worktree {
        key: String,
    },
    /// Create an approval-gated follow-up job for a delivered change's revert.
    Revert {
        key: String,
    },
    Cancel {
        key: String,
        /// Also remove the isolated worktree and delete the Turtle Flow job branch.
        #[arg(long)]
        delete_branch: bool,
    },
}

pub async fn run(rt: &Runtime, command: JobCommandLine) -> Result<()> {
    match command {
        JobCommandLine::Create {
            title,
            project,
            description,
            file,
            job_type,
            priority,
            profile,
            budget,
            start,
        } => {
            let budget = budget
                .as_deref()
                .map(crate::cmd::budget::parse_tokens)
                .transpose()?;
            let description = match (description, file) {
                (_, Some(path)) => std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?,
                (Some(d), None) => d,
                (None, None) => String::new(),
            };
            let job_type: JobType = parse_label("job type", &job_type)?;
            let priority: Priority = parse_label("priority", &priority)?;
            let project = resolve_project(rt, project).await?;
            let profile_name = profile.unwrap_or_else(|| project.default_profile.clone());
            let new = NewJob {
                project_id: project.id,
                title,
                description,
                job_type,
                priority,
                profile: builtin_profile(&profile_name)?,
            };
            let record = rt
                .store
                .with(move |s| s.transaction(|tx| tx.create_job(&new, &Actor::local_user())))
                .await?;
            println!(
                "Created {} \"{}\" ({profile_name}).",
                record.key, record.title
            );
            if let Some(limit) = budget {
                let id = record.job.id;
                rt.store
                    .with(move |s| {
                        s.transaction(|tx| {
                            tx.set_budget(
                                BudgetScope::Job(id),
                                limit,
                                true,
                                Some(id),
                                &Actor::local_user(),
                            )
                        })
                    })
                    .await?;
                println!("Budget: {limit} tokens (the job waits for you when it is used up).");
            }
            if start {
                submit(rt, &record, JobCommand::Start).await?;
                println!(
                    "Started. The daemon picks it up: turtle job watch {}",
                    record.key
                );
            } else {
                println!("Start it with: turtle job start {}", record.key);
            }
            Ok(())
        }
        JobCommandLine::List { project, active } => {
            let project_id = match project {
                Some(slug) => Some(rt.store.with(move |s| s.project_by_slug(&slug)).await?.id),
                None => None,
            };
            let jobs = rt.store.with(move |s| s.list_jobs(project_id)).await?;
            let jobs: Vec<_> = jobs
                .into_iter()
                .filter(|j| !active || !j.job.state.is_terminal())
                .collect();
            if jobs.is_empty() {
                println!("No jobs.");
            }
            for j in jobs {
                println!(
                    "{:<10} {:<20} {:<8} {}",
                    j.key.to_string(),
                    label(&j.job.state),
                    label(&j.priority),
                    j.title
                );
            }
            Ok(())
        }
        JobCommandLine::Show { key } => show(rt, &key).await,
        JobCommandLine::Events { key, json } => {
            let job = find(rt, &key).await?;
            let id = job.job.id;
            let events = rt.store.with(move |s| s.list_events(Some(id))).await?;
            for e in events {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "seq": e.seq, "at": e.occurred_at, "kind": e.kind,
                            "actor": format!("{}:{}", e.actor_kind, e.actor_id),
                            "payload": e.payload, "hash": e.hash,
                        })
                    );
                } else {
                    println!(
                        "{:>5}  {}  {:<28} {}:{}",
                        e.seq, e.occurred_at, e.kind, e.actor_kind, e.actor_id
                    );
                }
            }
            Ok(())
        }
        JobCommandLine::Watch { key } => watch(rt, &key).await,
        JobCommandLine::Start { key } => act(rt, &key, JobCommand::Start, "Started").await,
        JobCommandLine::Pause { key, hard } => {
            let mode = if hard {
                PauseMode::Hard
            } else {
                PauseMode::Soft
            };
            act(rt, &key, JobCommand::Pause { mode }, "Paused").await
        }
        JobCommandLine::Resume { key } => act(rt, &key, JobCommand::Resume, "Resumed").await,
        JobCommandLine::Resolve { key, target } => resolve(rt, &key, target.as_deref()).await,
        JobCommandLine::Sync { key } => sync(rt, &key).await,
        JobCommandLine::Worktree { key } => worktree_path(rt, &key).await,
        JobCommandLine::Revert { key } => revert(rt, &key).await,
        JobCommandLine::Cancel { key, delete_branch } => cancel(rt, &key, delete_branch).await,
    }
}

async fn worktree_path(rt: &Runtime, key: &str) -> Result<()> {
    let job = find(rt, key).await?;
    let id = job.job.id;
    let project = rt.store.with(move |s| s.project_of_job(id)).await?;
    let base = match job.base_commit.as_ref() {
        Some(base) => base.clone(),
        None => {
            let repo = rt.cfg.repo(&project);
            let branch = project
                .settings
                .target_branch
                .as_deref()
                .unwrap_or(&project.default_branch);
            repo.branch_tip(branch)
                .await?
                .ok_or_else(|| anyhow::anyhow!("target branch `{branch}` does not exist"))?
        }
    };
    let layout = Layout::new(&rt.cfg, &project, &job, &base)?;
    println!("{}", layout.path.display());
    Ok(())
}

async fn cancel(rt: &Runtime, key: &str, delete_branch: bool) -> Result<()> {
    let job = find(rt, key).await?;
    if job.job.state.is_terminal() {
        bail!(
            "cannot cancel {}; job is already {}",
            job.key,
            label(&job.job.state)
        );
    }
    let command = if delete_branch {
        JobCommand::CancelAndDeleteBranch
    } else {
        JobCommand::Cancel
    };
    let after = submit(rt, &job, command).await?;
    if delete_branch {
        println!(
            "Cancelled {}: worktree cleanup and job-branch deletion are queued for the daemon.",
            after.key
        );
    } else {
        println!("Cancelled {}: now {}.", after.key, label(&after.job.state));
    }
    Ok(())
}

async fn revert(rt: &Runtime, key: &str) -> Result<()> {
    let original = find(rt, key).await?;
    let id = original.job.id;
    let delivery = rt
        .store
        .with(move |s| s.delivery(id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("job {key} has no delivered commit to revert"))?;
    let commit = delivery
        .result_commit
        .ok_or_else(|| anyhow::anyhow!("job {key} has no recorded delivery commit"))?;
    if original.job.state != JobState::Done {
        bail!("job {key} is not delivered; only completed delivery commits can be reverted");
    }
    let project = rt.store.with(move |s| s.project_of_job(id)).await?;
    let new = NewJob {
        project_id: project.id,
        title: format!("Revert {}", original.title),
        description: format!(
            "Prepare a follow-up change to revert delivery commit {commit}. Review, verify, approve, and deliver it through the normal workflow."
        ),
        job_type: JobType::Chore,
        priority: Priority::Normal,
        profile: builtin_profile(&project.default_profile)?,
    };
    let created = rt
        .store
        .with(move |s| s.transaction(|tx| tx.create_job(&new, &Actor::local_user())))
        .await?;
    println!(
        "Created {} as an approval-gated revert job for {commit}.",
        created.key
    );
    println!("Start it with: turtle job start {}", created.key);
    Ok(())
}

async fn resolve(rt: &Runtime, key: &str, target: Option<&str>) -> Result<()> {
    let job = find(rt, key).await?;
    let blocked = job
        .job
        .blocked
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("job is not blocked"))?;
    if blocked.reason == turtle_workflow::BlockReason::ReviewDecisionRequired {
        let target: JobState =
            parse_label("resolution target", target.unwrap_or("implementation"))?;
        let after = rt
            .engine
            .submit(
                job.job.id,
                JobCommand::ResolveBlock { target },
                Actor::local_user(),
            )
            .await?;
        println!("Resolved {}: now {}.", after.key, label(&after.job.state));
        return Ok(());
    }
    if blocked.reason != turtle_workflow::BlockReason::ConflictDetected {
        bail!("job is blocked for another reason: {:?}", blocked.reason);
    }
    if target.is_some_and(|value| value != "build_test") {
        bail!("conflict resolution continues at build_test");
    }
    let id = job.job.id;
    let project = rt.store.with(move |s| s.project_of_job(id)).await?;
    let base = job
        .base_commit
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("job has no base commit"))?;
    let layout = Layout::new(&rt.cfg, &project, &job, base)?;
    let repo = rt.cfg.repo(&project);
    let wt = repo.at(&layout.path);
    let tip = repo
        .branch_tip(&layout.target_branch)
        .await?
        .ok_or_else(|| anyhow::anyhow!("target branch is missing"))?;
    let head = wt.head().await?;
    if !wt.is_clean().await? || !repo.is_ancestor(&tip, &head).await? {
        bail!(
            "incorporate `{}` into `{}` and commit the resolved changes in {} first; then run `turtle job resolve {}`",
            layout.target_branch,
            layout.spec.branch,
            layout.path.display(),
            job.key
        );
    }
    if base != tip {
        let old = base.to_owned();
        let new = tip.clone();
        rt.store
            .with(move |s| {
                s.transaction(|tx| {
                    tx.refresh_base_commit(id, &old, &new)?;
                    tx.invalidate_approvals(
                        id,
                        &[turtle_domain::GateKind::Acceptance],
                        "base_refreshed",
                    )?;
                    Ok(())
                })
            })
            .await?;
    }
    let after = rt
        .engine
        .submit(
            id,
            JobCommand::ResolveBlock {
                target: JobState::BuildTest,
            },
            Actor::local_user(),
        )
        .await?;
    println!(
        "Resolved {}: now {}. Gates, review, verification and acceptance will run again.",
        after.key,
        label(&after.job.state)
    );
    Ok(())
}

async fn sync(rt: &Runtime, key: &str) -> Result<()> {
    let job = find(rt, key).await?;
    let id = job.job.id;
    let delivery = rt
        .store
        .with(move |s| s.delivery(id))
        .await?
        .ok_or_else(|| anyhow::anyhow!("job has no delivery record"))?;
    if delivery.mode != "pr" {
        bail!("job was not delivered through a pull request");
    }
    if job.job.state == JobState::Done {
        println!("{} is already delivered.", job.key);
        return Ok(());
    }
    if job.job.state != JobState::Delivery {
        bail!("job is not waiting for a pull request merge");
    }
    let url = delivery
        .pr_url
        .ok_or_else(|| anyhow::anyhow!("PR URL is missing"))?;
    let project = rt.store.with(move |s| s.project_of_job(id)).await?;
    let info = PrClient::new(rt.cfg.gh_program.clone())
        .view(std::path::Path::new(&project.repo_path), &url)
        .await
        .map_err(anyhow::Error::msg)?;
    if info.url != url {
        bail!("gh returned a different PR URL");
    }
    if info.head_ref_oid.as_deref() != delivery.result_commit.as_deref() {
        bail!("the PR head changed after acceptance; inspect it before marking delivery complete");
    }
    if !info.merged() {
        println!("{} is still waiting for PR merge: {}", job.key, url);
        return Ok(());
    }
    let u = url.clone();
    rt.store
        .with(move |s| s.transaction(|tx| tx.mark_pr_merged(id, &u)))
        .await?;
    let after = rt
        .engine
        .submit(id, JobCommand::Delivered, Actor::Scheduler)
        .await?;
    println!(
        "Delivered {} via {}: now {}.",
        after.key,
        url,
        label(&after.job.state)
    );
    Ok(())
}

fn builtin_profile(name: &str) -> Result<Profile> {
    let base = match name {
        "fast" => ProfileBase::Fast,
        "standard" => ProfileBase::Standard,
        "strict" => ProfileBase::Strict,
        other => bail!("unknown profile `{other}`"),
    };
    Ok(Profile::builtin(base))
}

async fn resolve_project(rt: &Runtime, slug: Option<String>) -> Result<ProjectRecord> {
    match slug {
        Some(slug) => Ok(rt.store.with(move |s| s.project_by_slug(&slug)).await?),
        None => {
            let mut all = rt.store.with(|s| s.list_projects()).await?;
            match all.len() {
                0 => bail!("no projects yet; register one with: turtle project add <path>"),
                1 => Ok(all.remove(0)),
                _ => bail!(
                    "several projects exist; choose one with -p ({})",
                    all.iter()
                        .map(|p| p.slug.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            }
        }
    }
}

pub async fn find(rt: &Runtime, key: &str) -> Result<JobRecord> {
    let key: JobKey = key
        .to_ascii_uppercase()
        .parse()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(rt.store.with(move |s| s.find_job(&key)).await?)
}

async fn submit(rt: &Runtime, job: &JobRecord, command: JobCommand) -> Result<JobRecord> {
    Ok(rt
        .engine
        .submit(job.job.id, command, Actor::local_user())
        .await?)
}

async fn act(rt: &Runtime, key: &str, command: JobCommand, done: &str) -> Result<()> {
    let job = find(rt, key).await?;
    let after = submit(rt, &job, command).await?;
    println!("{done} {}: now {}.", after.key, label(&after.job.state));
    Ok(())
}

async fn show(rt: &Runtime, key: &str) -> Result<()> {
    let job = find(rt, key).await?;
    let id = job.job.id;
    let project = rt.store.with(move |s| s.project_of_job(id)).await?;
    let (approvals, events, delivery, gate_revision) = rt
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.list_approvals(id)?,
                s.list_events(Some(id))?,
                s.delivery(id)?,
                s.latest_revision(id, ArtifactKind::GateResult)?,
            ))
        })
        .await?;
    if let Some(revision) = gate_revision {
        let bytes = rt.blobs.get(&revision.sha256)?;
        let gates: serde_json::Value = serde_json::from_slice(&bytes)?;
        println!(
            "  gates        {}",
            if gates["passed"] == true {
                "passed"
            } else {
                "failed"
            }
        );
        if let Some(results) = gates["results"].as_array() {
            for result in results {
                println!(
                    "    {:<22} {}",
                    result["gate"].as_str().unwrap_or("unknown"),
                    result["outcome"].as_str().unwrap_or("unknown")
                );
            }
        }
        if let Some(flags) = gates["acknowledgement_flags"].as_array() {
            for flag in flags.iter().filter_map(serde_json::Value::as_str) {
                println!("    acknowledgement      {flag}");
            }
        }
    }
    println!("{}  {}", job.key, job.title);
    println!("  project      {} ({})", project.name, project.slug);
    println!("  state        {}", label(&job.job.state));
    println!("  profile      {}", job.job.profile.name);
    println!(
        "  type         {}   priority {}",
        label(&job.job_type),
        label(&job.priority)
    );
    if let Some(b) = &job.job.blocked {
        println!(
            "  blocked      {}",
            serde_json::to_string(b).unwrap_or_default()
        );
        if b.reason == turtle_workflow::BlockReason::ConflictDetected {
            println!(
                "  resolution   incorporate the current target branch into the job branch, commit the resolution, then run `turtle job resolve {}`",
                job.key
            );
        } else if b.reason == turtle_workflow::BlockReason::ReviewDecisionRequired {
            println!(
                "  resolution   inspect the review findings, then run `turtle job resolve {}` to authorize a fix",
                job.key
            );
        }
    }
    if !job.job.holds.is_empty() {
        println!(
            "  holds        {}",
            serde_json::to_string(&job.job.holds).unwrap_or_default()
        );
        for hold in &job.job.holds {
            if let Hold::RateLimited { provider } = hold {
                let limit = rt
                    .store
                    .with({
                        let provider = provider.clone();
                        move |s| s.limited_provider(&provider)
                    })
                    .await?;
                let explanation = limit.and_then(|limit| limit.reset_at).map_or_else(
                    || format!("waiting for {provider}'s limit to reset (reset unknown)"),
                    |reset| {
                        format!(
                            "waiting for {provider}'s limit to reset at {}",
                            crate::display_local_time(&reset)
                        )
                    },
                );
                println!("  waiting      {explanation}");
            }
        }
    }
    let mut delivery_hint = None;
    if let Some(d) = delivery {
        println!(
            "  delivery     {} on {} {}",
            d.status,
            d.target_branch,
            d.result_commit.as_deref().unwrap_or("")
        );
        if let Some(url) = &d.pr_url {
            println!("  pull request {url}");
            if d.status == "pending" {
                println!(
                    "  waiting      merge the PR, then run `turtle job sync {}`",
                    job.key
                );
            }
        }
        if d.status == "pending_integration" {
            delivery_hint = Some(format!(
                "               `{}` is checked out, so the result waits at refs/turtle/delivery/{}\n               Integrate it yourself: git merge --ff-only refs/turtle/delivery/{}",
                d.target_branch, job.key, job.key
            ));
        }
    }
    if let Some(d) = &delivery_hint {
        println!("{d}");
    }
    for a in approvals {
        println!(
            "  approval     {} {} {} {}",
            a.id,
            label(&a.gate),
            label(&a.status),
            a.subject_hash.short()
        );
    }
    let denied: Vec<_> = events
        .iter()
        .filter(|event| event.kind == "run.policy_denied")
        .collect();
    if !denied.is_empty() {
        println!("  denied actions {}", denied.len());
        for event in denied {
            let tool = event
                .payload
                .get("denial")
                .and_then(|d| d.get("tool"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown tool");
            println!(
                "    {} (run {})",
                tool,
                event
                    .payload
                    .get("run")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown")
            );
        }
    }
    let usage = rt.store.with(move |s| s.job_usage(id)).await?;
    if usage.runs > 0 {
        println!(
            "  tokens       {} measured, {} estimated over {} runs{}",
            usage.tokens_reported,
            usage.tokens_estimated,
            usage.runs,
            if usage.runs_unavailable > 0 {
                format!(" ({} with unknown usage)", usage.runs_unavailable)
            } else {
                String::new()
            }
        );
    }
    if let Some(report) = crate::cmd::budget::preflight_for_job(rt, &job).await? {
        println!(
            "  estimate     {}–{} tokens (most likely {}, {} confidence)",
            report.estimate.p10,
            report.estimate.p90,
            report.estimate.p50,
            report.estimate.confidence.label()
        );
    }
    if job
        .job
        .holds
        .contains(&turtle_workflow::Hold::AwaitingBudget)
    {
        println!(
            "  waiting      out of budget: turtle budget set <tokens> --job {}",
            job.key
        );
    }
    println!(
        "  events       {} (turtle job events {})",
        events.len(),
        job.key
    );
    Ok(())
}

async fn watch(rt: &Runtime, key: &str) -> Result<()> {
    let job = find(rt, key).await?;
    let id = job.job.id;
    let mut seen = 0_i64;
    loop {
        let events = rt.store.with(move |s| s.list_events(Some(id))).await?;
        let floor = seen;
        for e in events.iter().filter(|e| e.seq > floor) {
            println!(
                "{:>5}  {:<28} {}:{}",
                e.seq, e.kind, e.actor_kind, e.actor_id
            );
            seen = e.seq;
        }
        let now = rt.store.with(move |s| s.load_job(id)).await?;
        let state = now.job.state;
        if state.is_terminal() || state.is_approval() || state == JobState::Blocked {
            println!("-> {}", label(&state));
            if state.is_approval() {
                println!("   Needs you: turtle approvals");
            } else if state == JobState::Blocked {
                println!("   Details: turtle job show {}", now.key);
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
