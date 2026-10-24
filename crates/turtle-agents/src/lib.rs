//! Provider adapters. This first increment contains CLI discovery and the output
//! parsers verified against real transcripts (spike S-1/S-2, `spikes/fixtures/`).
//! The full `ProviderAdapter` trait follows `docs/AGENT-PROTOCOL.md` §3.

pub mod claude;
pub mod codex;
pub mod discovery;
pub mod runner;

pub use discovery::{CompatStatus, ProviderId, ProviderInfo, discover};
pub use runner::{
    AgentError, AgentOutput, AgentRequest, AgentRunner, FakeRunner, RecordedCall, Step,
};
