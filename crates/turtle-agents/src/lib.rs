//! Provider adapters: CLI discovery, output parsers verified against real transcripts (spike
//! S-1/S-2, `spikes/fixtures/`), the Claude Code runner with its role prompts and document
//! schemas, and a scripted `FakeRunner`. The Codex runner and the full `ProviderAdapter` trait
//! (capabilities, cancellation) follow `docs/AGENT-PROTOCOL.md` §3.

pub mod claude;
pub mod claude_runner;
pub mod codex;
pub mod discovery;
pub mod prompts;
pub mod runner;
pub mod schemas;

pub use claude_runner::{ClaudeConfig, ClaudeRunner};
pub use discovery::{CompatStatus, ProviderId, ProviderInfo, discover};
pub use runner::{
    AgentContext, AgentError, AgentOutput, AgentRequest, AgentRunner, FakeRunner, RecordedCall,
    Step,
};
