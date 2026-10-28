//! Policy logic with no IO: approval binding hashes and invalidation rules.
//! Protected paths, security-policy precedence and entitlements follow in later PRs.

pub mod binding;
pub mod invalidation;

pub use binding::{
    ApprovalBinding, ArtifactRef, BINDING_SCHEMA, BudgetEnvelope, BudgetIncrease, GitBinding,
};
pub use invalidation::MaterialChange;
