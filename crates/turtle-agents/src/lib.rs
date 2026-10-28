//! Provider adapters: CLI discovery, output parsers verified against real transcripts (spike
//! S-1/S-2, `docs/spikes/fixtures/`), Claude Code and Codex runners with role prompts and document
//! schemas, capability discovery, ordered routing, and a scripted `FakeRunner`. The larger
//! `ProviderAdapter` contract (health caching, cancellation) remains in the design.

pub mod claude;
pub mod claude_runner;
pub mod codex;
pub mod codex_runner;
pub mod discovery;
pub mod prompts;
pub mod runner;
pub mod schemas;
pub mod snapshot;

pub use claude::PermissionDenial;
pub use claude_runner::{ClaudeConfig, ClaudeRunner};
pub use codex::local_offset;
pub use codex_runner::{CodexConfig, CodexRunner};
pub use discovery::{
    AuthStatus, CapabilityRegistry, CompatStatus, Discovered, ProviderCapabilities, ProviderId,
    ProviderInfo, discover, discover_capabilities, discover_with_snapshot,
};
pub use runner::{
    AgentContext, AgentError, AgentOutput, AgentRequest, AgentRunner, FakeRunner, Independence,
    RecordedCall, RouteContext, RouteSelection, Step,
};
pub use snapshot::{
    ADAPTER_VERSION, CapabilityChange, CapabilityDiff, CapabilitySet, StoredSnapshot, VersionChange,
};
pub mod routing;
pub use routing::{ProviderSlot, RoutingRunner};
