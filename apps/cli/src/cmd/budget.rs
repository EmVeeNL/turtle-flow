//! `turtle budget show | set | estimate`: budgets and planned-job estimates.

use anyhow::{Result, bail};
use clap::Subcommand;
use turtle_domain::{Actor, ArtifactKind, Plan, Role, Specification};
use turtle_scheduler::lift_budget_hold;
use turtle_store::{BudgetRecord, BudgetScope};
use turtle_tokens::{
    BucketPath, BudgetInput, Decision, Preflight, cold_start_estimate, estimate, preflight,
    score_complexity,
};
use turtle_workflow::Hold;

use crate::cmd::job;
use crate::runtime::Runtime;

#[derive(Debug, Subcommand)]
pub enum BudgetCommand {
    /// Show budgets and what has been spent against them.
    Show,
    /// Estimate a planned job's token range and budget decision.
    Estimate { job_key: String },
    /// Set or change a token limit. Raising the limit of a held job resumes it.
    Set {
        /// Limit in tokens processed, e.g. `400k` or `20M`.
        tokens: String,
        /// Budget for one job (its lifetime), e.g. `--job PAY-1`.
        #[arg(long, conflicts_with_all = ["project", "global"])]
        job: Option<String>,
        /// Monthly budget for a project, by slug.
        #[arg(short, long, conflicts_with = "global")]
        project: Option<String>,
        /// Monthly budget for everything Turtle Flow does.
        #[arg(long)]
        global: bool,
        /// Keep going and only warn when the limit is passed (default: stop).
        #[arg(long)]
        soft: bool,
    },
}

/// Parses `400k`, `1.5M`, `20_000` into a token count.
pub fn parse_tokens(text: &str) -> Result<u64> {
    let t = text.trim().replace('_', "");
    let (number, factor) = match t.chars().last() {
        Some('k' | 'K') => (&t[..t.len() - 1], 1_000.0),
        Some('m' | 'M') => (&t[..t.len() - 1], 1_000_000.0),
        _ => (t.as_str(), 1.0),
    };
    let value: f64 = number
        .parse()
        .map_err(|_| anyhow::anyhow!("`{text}` is not a token amount (try 400k or 20M)"))?;
    if !value.is_finite() || value <= 0.0 {
        bail!("a budget must be more than zero tokens");
    }
    // Rounding to whole tokens is the intent; amounts here are far below f64's integer limit.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Ok((value * factor).round() as u64)
}

fn tokens(n: u64) -> String {
    match n {
        0..=9_999 => n.to_string(),
        10_000..=9_999_999 => format!("{:.1}k", n as f64 / 1_000.0),
        _ => format!("{:.2}M", n as f64 / 1_000_000.0),
    }
}

pub async fn run(rt: &Runtime, command: BudgetCommand) -> Result<()> {
    match command {
        BudgetCommand::Show => show(rt).await,
        BudgetCommand::Estimate { job_key } => {
            let job = job::find(rt, &job_key).await?;
            if let Some(report) = preflight_for_job(rt, &job).await? {
                let decision = match report.decision {
                    Decision::Allow => "allow",
                    Decision::Warn => "warn",
                    Decision::RequireApproval => "needs approval",
                    Decision::Deny => "deny",
                };
                println!(
                    "{}: {} (complexity {:?}, score {})",
                    job.key, decision, report.complexity.class, report.complexity.score
                );
                println!(
                    "  estimate     {}–{} tokens (most likely {})",
                    report.estimate.p10, report.estimate.p90, report.estimate.p50
                );
                println!("  confidence   {}", report.estimate.confidence.label());
                println!("  envelope     {} tokens", report.envelope_tokens);
                println!("  reason       {}", report.reasons.join("; "));
                println!("  scope        planned runs only; route and retry costs unverified");
            } else {
                println!(
                    "{}: estimate unknown until a specification and plan exist.",
                    job.key
                );
            }
            Ok(())
        }
        BudgetCommand::Set {
            tokens: amount,
            job: job_key,
            project,
            global,
            soft,
        } => {
            let limit = parse_tokens(&amount)?;
            let hard = !soft;
            let (scope, job_id, jobs_to_wake) = match (job_key, project, global) {
                (Some(key), None, false) => {
                    let j = job::find(rt, &key).await?;
                    (BudgetScope::Job(j.job.id), Some(j.job.id), vec![j.job.id])
                }
                (None, Some(slug), false) => {
                    let p = rt.store.with(move |s| s.project_by_slug(&slug)).await?;
                    let pid = p.id;
                    let jobs = rt.store.with(move |s| s.list_jobs(Some(pid))).await?;
                    (
                        BudgetScope::Project(p.id),
                        None,
                        jobs.into_iter().map(|j| j.job.id).collect(),
                    )
                }
                (None, None, true) => {
                    let jobs = rt.store.with(|s| s.list_jobs(None)).await?;
                    (
                        BudgetScope::Global,
                        None,
                        jobs.into_iter().map(|j| j.job.id).collect(),
                    )
                }
                _ => bail!("say which budget: --job <key>, -p <project> or --global"),
            };
            let record = rt
                .store
                .with(move |s| {
                    s.transaction(|tx| {
                        tx.set_budget(scope, limit, hard, job_id, &Actor::local_user())
                    })
                })
                .await?;
            println!(
                "{} budget is now {} tokens ({}); {} spent, {} reserved.",
                scope.name(),
                tokens(record.limit_tokens),
                if record.hard {
                    "stops at the limit"
                } else {
                    "warns only"
                },
                tokens(record.settled_tokens),
                tokens(record.reserved_tokens)
            );
            for id in jobs_to_wake {
                if lift_budget_hold(&rt.engine, id).await? {
                    let key = rt.store.with(move |s| s.load_job(id)).await?.key;
                    println!("{key} was waiting for budget and will continue.");
                }
            }
            Ok(())
        }
    }
}

pub async fn preflight_for_job(
    rt: &Runtime,
    job: &turtle_store::JobRecord,
) -> Result<Option<Preflight>> {
    let id = job.job.id;
    let (spec_rev, plan_rev) = rt
        .store
        .with(move |s| {
            Ok::<_, turtle_store::StoreError>((
                s.latest_revision(id, ArtifactKind::Specification)?,
                s.latest_revision(id, ArtifactKind::Plan)?,
            ))
        })
        .await?;
    let (Some(spec_rev), Some(plan_rev)) = (spec_rev, plan_rev) else {
        return Ok(None);
    };
    let spec: Specification = serde_json::from_slice(&rt.blobs.get(&spec_rev.sha256)?)?;
    let plan: Plan = serde_json::from_slice(&rt.blobs.get(&plan_rev.sha256)?)?;
    let mut roles = vec![Role::SpecWriter, Role::Planner];
    if job.job.profile.discovery_enabled(job.job.complexity) {
        roles.insert(0, Role::Discovery);
    }
    roles.extend(std::iter::repeat_n(Role::Implementer, plan.tasks.len()));
    roles.extend([Role::Reviewer, Role::Verifier]);
    let mut runs = Vec::with_capacity(roles.len());
    for role in roles {
        let complexity = match role {
            Role::SpecWriter => turtle_domain::Complexity::XS,
            Role::Implementer | Role::Fixer => score_complexity(&spec, Some(&plan)).class,
            _ => score_complexity(&spec, None).class,
        };
        let path = BucketPath::new(&job.project_id.to_string(), role, complexity, "", "");
        let buckets = rt.store.with(move |s| s.estimation_buckets(&path)).await?;
        // The CLI has no provisional route. Only the route-independent role/class level is safe.
        let broad = [None, None, None, buckets[3].clone()];
        runs.push((role, estimate(&broad, cold_start_estimate(role))));
    }
    let budgets = rt
        .store
        .with(move |s| s.transaction(|tx| tx.budgets_for_job(id)))
        .await?;
    let hard_headroom = budgets
        .iter()
        .filter(|b| b.hard)
        .map(turtle_store::BudgetRecord::headroom)
        .min();
    let soft_headroom = budgets
        .iter()
        .filter(|b| !b.hard)
        .map(turtle_store::BudgetRecord::headroom)
        .min();
    let project_remaining = budgets
        .iter()
        .find(|b| matches!(b.scope, turtle_store::BudgetScope::Project(_)))
        .map(turtle_store::BudgetRecord::headroom);
    Ok(Some(preflight(
        &spec,
        &plan,
        &runs,
        &BudgetInput {
            hard_headroom,
            soft_headroom,
            project_remaining,
            max_without_approval: None,
            provider_available: true,
        },
    )))
}

async fn show(rt: &Runtime) -> Result<()> {
    let budgets = rt.store.with(|s| s.list_budgets()).await?;
    if budgets.is_empty() {
        println!("No budgets set. Runs are recorded but not limited.");
        println!("Set one with: turtle budget set 20M -p <project>   (or --job <key>, --global)");
        return Ok(());
    }
    for b in budgets {
        let name = scope_name(rt, &b).await?;
        println!(
            "{:<22} {:<9} limit {:>8}  spent {:>8}  reserved {:>8}  {}",
            name,
            b.period_start.as_deref().map_or("lifetime", |p| &p[..7]),
            tokens(b.limit_tokens),
            tokens(b.settled_tokens),
            tokens(b.reserved_tokens),
            if b.hard { "hard" } else { "soft" }
        );
    }
    let held = rt
        .store
        .with(|s| {
            Ok::<_, turtle_store::StoreError>(
                s.list_jobs(None)?
                    .into_iter()
                    .filter(|j| j.job.holds.contains(&Hold::AwaitingBudget))
                    .map(|j| j.key.to_string())
                    .collect::<Vec<_>>(),
            )
        })
        .await?;
    if !held.is_empty() {
        println!("\nWaiting for budget: {}", held.join(", "));
    }
    Ok(())
}

async fn scope_name(rt: &Runtime, b: &BudgetRecord) -> Result<String> {
    Ok(match b.scope {
        BudgetScope::Global => "global".to_owned(),
        BudgetScope::Project(id) => {
            let p = rt.store.with(move |s| s.load_project(id)).await?;
            format!("project {}", p.slug)
        }
        BudgetScope::Job(id) => {
            let j = rt.store.with(move |s| s.load_job(id)).await?;
            format!("job {}", j.key)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_amounts_accept_suffixes_and_reject_nonsense() {
        assert_eq!(parse_tokens("400k").expect("k"), 400_000);
        assert_eq!(parse_tokens("1.5M").expect("m"), 1_500_000);
        assert_eq!(parse_tokens("20_000").expect("plain"), 20_000);
        assert!(parse_tokens("0").is_err());
        assert!(parse_tokens("-5k").is_err());
        assert!(parse_tokens("lots").is_err());
        assert!(parse_tokens("").is_err());
    }
}
