//! Periodically clears provider holds only after their reset, or a conservative hour for unknowns.

use turtle_domain::Actor;
use turtle_workflow::{Hold, JobCommand};

use crate::engine::{ContextProvider, Engine, EngineError};

pub async fn lift_rate_limit_holds<C: ContextProvider>(
    engine: &Engine<C>,
) -> Result<usize, EngineError> {
    let providers = engine
        .store()
        .with(|store| store.providers_ready_to_clear())
        .await?;
    let mut lifted = 0;
    for provider in providers {
        let jobs = engine.store().with(|store| store.list_jobs(None)).await?;
        for job in jobs {
            if job.job.holds.contains(&Hold::RateLimited {
                provider: provider.clone(),
            }) {
                engine
                    .submit(
                        job.job.id,
                        JobCommand::RateLimitCleared {
                            provider: provider.clone(),
                        },
                        Actor::Scheduler,
                    )
                    .await?;
                lifted += 1;
            }
        }
        engine
            .store()
            .with(move |store| store.transaction(|tx| tx.resolve_provider_limits(&provider)))
            .await?;
    }
    Ok(lifted)
}
