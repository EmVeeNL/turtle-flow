//! Capability discovery with persisted snapshots. Each provider's latest snapshot is reused while
//! its CLI version is unchanged; a changed version (or `force`) re-discovers and records a new
//! snapshot, which runs started afterwards link to. A missing or unparseable binary records
//! nothing and keeps the previous snapshot as history.

use anyhow::Result;
use turtle_agents::{CapabilityRegistry, ProviderCapabilities, ProviderId, StoredSnapshot};
use turtle_scheduler::SharedStore;
use turtle_store::CapabilitySnapshot;

#[derive(Debug)]
pub struct Discovery {
    pub provider: ProviderId,
    /// The provider's latest snapshot after discovery, if any was ever recorded.
    pub snapshot: Option<CapabilitySnapshot>,
    /// A new snapshot was written by this discovery.
    pub recorded: bool,
}

pub async fn discover(
    store: &SharedStore,
    force: bool,
) -> Result<(CapabilityRegistry, Vec<Discovery>)> {
    let mut capabilities = Vec::new();
    let mut outcomes = Vec::new();
    for provider in ProviderId::ALL {
        let stored = store
            .with(move |s| s.capability_snapshots(provider.as_str(), 1))
            .await?
            .pop();
        let discovered = turtle_agents::discover_with_snapshot(
            provider,
            stored.as_ref().map(|s| StoredSnapshot {
                cli_version: s.cli_version.as_deref(),
                capabilities: &s.capabilities,
            }),
            force,
        )
        .await;
        let (snapshot, recorded) = if discovered.reused {
            (stored, false)
        } else {
            record(store, &discovered.capabilities, stored).await?
        };
        outcomes.push(Discovery {
            provider,
            snapshot,
            recorded,
        });
        capabilities.push(discovered.capabilities);
    }
    Ok((
        CapabilityRegistry::from_capabilities(capabilities),
        outcomes,
    ))
}

async fn record(
    store: &SharedStore,
    caps: &ProviderCapabilities,
    stored: Option<CapabilitySnapshot>,
) -> Result<(Option<CapabilitySnapshot>, bool)> {
    let Some(cli_version) = caps.cli_version() else {
        return Ok((stored, false));
    };
    let provider = caps.info.provider.as_str();
    let set = caps.capability_set().to_json();
    let (snapshot, recorded) = store
        .with(move |s| {
            s.transaction(|tx| tx.record_capability_snapshot(provider, Some(&cli_version), &set))
        })
        .await?;
    Ok((Some(snapshot), recorded))
}

/// `turtle providers refresh`: forced re-discovery, reporting each provider's snapshot.
pub async fn refresh(store: &SharedStore) -> Result<()> {
    let (registry, outcomes) = discover(store, true).await?;
    for outcome in outcomes {
        let caps = registry.get(outcome.provider);
        let name = outcome.provider.as_str();
        match (&outcome.snapshot, caps.cli_version()) {
            (_, None) => println!(
                "{name}: not recorded ({}){}",
                caps.info.detail,
                outcome
                    .snapshot
                    .as_ref()
                    .map_or_else(String::new, |s| format!(
                        "; last snapshot #{} ({})",
                        s.version,
                        s.cli_version.as_deref().unwrap_or("unknown")
                    ))
            ),
            (Some(s), Some(version)) => println!(
                "{name}: snapshot #{} {} (cli {version}, {})",
                s.version,
                if outcome.recorded {
                    "recorded"
                } else {
                    "unchanged"
                },
                short_hash(&s.content_hash)
            ),
            (None, Some(version)) => println!("{name}: not recorded (cli {version})"),
        }
    }
    Ok(())
}

#[must_use]
pub fn short_hash(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}
