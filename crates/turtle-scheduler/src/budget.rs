//! Lifting a budget hold once there is room again.

use turtle_domain::{Actor, JobId};
use turtle_workflow::{Hold, JobCommand};

use crate::engine::{ContextProvider, Engine, EngineError};

/// Smallest headroom worth waking a held job for: the cheapest run's cold-start estimate.
const MIN_HEADROOM: u64 = 10_000;

/// Lifts the job's `AwaitingBudget` hold if every hard budget that applies has room again.
/// Returns whether the hold was lifted. `BudgetRestored` is a machine result, so it is issued
/// by the scheduler as a consequence of the budget change, never by a user directly.
pub async fn lift_budget_hold<C: ContextProvider>(
    engine: &Engine<C>,
    job: JobId,
) -> Result<bool, EngineError> {
    let record = engine.store().with(move |s| s.load_job(job)).await?;
    if !record.job.holds.contains(&Hold::AwaitingBudget) {
        return Ok(false);
    }
    let budgets = engine
        .store()
        .with(move |s| s.transaction(|tx| tx.budgets_for_job(job)))
        .await?;
    if budgets
        .iter()
        .any(|b| b.hard && b.headroom() < MIN_HEADROOM)
    {
        return Ok(false);
    }
    engine
        .submit(job, JobCommand::BudgetRestored, Actor::Scheduler)
        .await?;
    Ok(true)
}
