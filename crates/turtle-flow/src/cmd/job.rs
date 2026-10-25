//! `turtle job create | list | show | events | watch | start | pause | resume | cancel`.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use turtle_domain::{Actor, JobKey, JobState, JobType, Priority};
use turtle_store::{BudgetScope, JobRecord, NewJob, ProjectRecord};
use turtle_workflow::{JobCommand, PauseMode, Profile, ProfileBase};

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
    Cancel {
        key: String,
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
        JobCommandLine::Cancel { key } => act(rt, &key, JobCommand::Cancel, "Cancelled").await,
    }
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
    let (approvals, events, delivery) = rt
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.list_approvals(id)?,
                s.list_events(Some(id))?,
                s.delivery(id)?,
            ))
        })
        .await?;
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
    }
    if !job.job.holds.is_empty() {
        println!(
            "  holds        {}",
            serde_json::to_string(&job.job.holds).unwrap_or_default()
        );
    }
    let mut delivery_hint = None;
    if let Some(d) = delivery {
        println!(
            "  delivery     {} on {} {}",
            d.status,
            d.target_branch,
            d.result_commit.as_deref().unwrap_or("")
        );
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
