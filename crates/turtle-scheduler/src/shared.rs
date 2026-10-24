//! The store is synchronous (rusqlite). `SharedStore` serializes access to the single writer
//! connection and moves each call onto the blocking thread pool so async tasks never stall.

use std::sync::{Arc, Mutex, PoisonError};

use turtle_store::Store;

#[derive(Clone)]
pub struct SharedStore(Arc<Mutex<Store>>);

impl std::fmt::Debug for SharedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedStore").finish_non_exhaustive()
    }
}

impl SharedStore {
    #[must_use]
    pub fn new(store: Store) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    /// Runs `f` with exclusive access to the store on a blocking thread.
    ///
    /// A panic inside `f` rolls its transaction back (the `Tx` is dropped), so a poisoned lock
    /// still guards a consistent database and is safe to reuse.
    pub async fn with<T, F>(&self, f: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> T + Send + 'static,
    {
        let inner = Arc::clone(&self.0);
        let joined = tokio::task::spawn_blocking(move || {
            let mut guard = inner.lock().unwrap_or_else(PoisonError::into_inner);
            f(&mut guard)
        })
        .await;
        match joined {
            Ok(v) => v,
            Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
            Err(e) => panic!("store task was cancelled: {e}"),
        }
    }
}
