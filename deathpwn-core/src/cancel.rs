//! Cooperative cancellation primitive shared across the exec boundary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::Notify;

#[derive(Debug)]
struct Inner {
    cancelled: AtomicBool,
    notify: Notify,
}

/// A cheap, clonable cancellation handle. All clones share one state; calling
/// [`CancelToken::cancel`] on any clone flips the flag and wakes every task
/// currently awaiting [`CancelToken::cancelled`].
#[derive(Clone, Debug)]
pub struct CancelToken(Arc<Inner>);

impl CancelToken {
    /// Create a fresh, not-yet-cancelled token.
    pub fn new() -> Self {
        CancelToken(Arc::new(Inner {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }))
    }

    /// Request cancellation. Idempotent. Wakes all current waiters.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::SeqCst);
        self.0.notify.notify_waiters();
    }

    /// Whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::SeqCst)
    }

    /// Resolve as soon as cancellation is requested. Returns immediately if the
    /// token is already cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let notified = self.0.notify.notified();
        tokio::pin!(notified);
        // Register this waiter before re-checking the flag: if cancel() ran
        // between our first check and here, notify_waiters() would otherwise
        // have found no waiter and we would block forever.
        notified.as_mut().enable();

        if self.is_cancelled() {
            return;
        }

        notified.await;
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        CancelToken::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn new_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[tokio::test]
    async fn cancel_sets_flag_and_wakes_a_waiter() {
        let token = CancelToken::new();
        let waiter = token.clone();
        let handle = tokio::spawn(async move {
            waiter.cancelled().await;
        });

        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("cancelled() should resolve after cancel()")
            .expect("waiter task should not panic");
    }

    #[tokio::test]
    async fn cancelled_returns_immediately_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();
        tokio::time::timeout(Duration::from_millis(100), token.cancelled())
            .await
            .expect("already-cancelled token should resolve immediately");
    }

    #[tokio::test]
    async fn clones_share_state() {
        let a = CancelToken::new();
        let b = a.clone();
        a.cancel();
        assert!(b.is_cancelled());
    }
}
