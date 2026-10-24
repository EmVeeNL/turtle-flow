//! Crash injection (`docs/TESTING-STRATEGY.md` §6). The scheduler calls `CrashPlan::point` at
//! every step boundary where a real crash could leave work half done. In production the plan
//! never fires; tests fire it at the Nth boundary to simulate a process death there, restart,
//! and check that recovery converges.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("simulated crash at `{0}`")]
pub struct Crashed(pub String);

#[derive(Debug, Clone)]
pub struct CrashPlan(Arc<Inner>);

#[derive(Debug)]
struct Inner {
    crash_at: Option<usize>,
    seen: AtomicUsize,
    names: Mutex<Vec<String>>,
}

impl CrashPlan {
    /// The production plan: never crashes.
    #[must_use]
    pub fn never() -> Self {
        Self(Arc::new(Inner {
            crash_at: None,
            seen: AtomicUsize::new(0),
            names: Mutex::new(Vec::new()),
        }))
    }

    /// Crashes at the `n`th boundary (0-based) and only there.
    #[must_use]
    pub fn at(n: usize) -> Self {
        Self(Arc::new(Inner {
            crash_at: Some(n),
            seen: AtomicUsize::new(0),
            names: Mutex::new(Vec::new()),
        }))
    }

    /// A step boundary. Returns `Err` exactly when this is the planned crash point.
    pub fn point(&self, name: &str) -> Result<(), Crashed> {
        let index = self.0.seen.fetch_add(1, Ordering::SeqCst);
        self.0.names.lock().expect("lock").push(name.to_owned());
        if self.0.crash_at == Some(index) {
            Err(Crashed(format!("{name}#{index}")))
        } else {
            Ok(())
        }
    }

    /// Names of the boundaries passed so far, in order; the index is the boundary number.
    /// Tests use it to target every boundary of a given kind.
    #[must_use]
    pub fn boundary_names(&self) -> Vec<String> {
        self.0.names.lock().expect("lock").clone()
    }

    /// Number of boundaries passed so far (including the crash point).
    #[must_use]
    pub fn boundaries_seen(&self) -> usize {
        self.0.seen.load(Ordering::SeqCst)
    }

    /// Whether the planned crash point was reached.
    #[must_use]
    pub fn fired(&self) -> bool {
        self.0
            .crash_at
            .is_some_and(|n| self.0.seen.load(Ordering::SeqCst) > n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fires_once_at_the_chosen_boundary() {
        let plan = CrashPlan::at(2);
        assert!(plan.point("a").is_ok());
        assert!(plan.point("b").is_ok());
        assert_eq!(plan.point("c"), Err(Crashed("c#2".into())));
        assert!(plan.fired());
        assert!(plan.point("d").is_ok(), "only one crash");
        assert_eq!(plan.boundaries_seen(), 4);
    }

    #[test]
    fn never_does_not_fire() {
        let plan = CrashPlan::never();
        for _ in 0..100 {
            assert!(plan.point("x").is_ok());
        }
        assert!(!plan.fired());
    }
}
