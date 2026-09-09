//! Graceful shutdown.
//!
//! Guide Milestone 1 requires clean shutdown on SIGINT/SIGTERM, and it matters
//! more here than in most programs: a node killed mid-block must not leave a
//! partially-indexed block behind. The store's transaction boundary makes that
//! impossible at the database level, but a clean stop also means finishing the
//! block in progress rather than rolling it back, so a restart does not redo work.
//!
//! A second signal exits immediately — if the first shutdown is wedged, the
//! operator needs a way out that does not involve `SIGKILL`.

use std::sync::Arc;

use tokio::sync::Notify;

/// Broadcasts a shutdown request to every task.
#[derive(Clone)]
pub struct Shutdown {
    notify: Arc<Notify>,
}

impl Shutdown {
    /// Create a shutdown signal.
    pub fn new() -> Self {
        Self {
            notify: Arc::new(Notify::new()),
        }
    }

    /// Wait until shutdown is requested.
    ///
    /// `notified()` is permit-based, so a task that starts waiting after the
    /// signal fires still returns immediately rather than hanging.
    pub async fn recv(&self) {
        self.notify.notified().await;
    }

    /// Request shutdown, waking every waiter.
    pub fn trigger(&self) {
        self.notify.notify_waiters();
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

/// Wait for SIGINT or SIGTERM, then trigger `shutdown`.
///
/// A second signal aborts the process, so a stuck shutdown is still escapable.
pub async fn listen_for_signals(shutdown: Shutdown) {
    match wait_for_signal().await {
        Ok(name) => {
            tracing::info!(signal = name, "shutdown requested, finishing current block");
            shutdown.trigger();
        }
        Err(e) => {
            tracing::error!(error = %e, "could not install signal handlers");
            return;
        }
    }

    if let Ok(name) = wait_for_signal().await {
        tracing::warn!(signal = name, "second signal received, exiting immediately");
        std::process::exit(130);
    }
}

#[cfg(unix)]
async fn wait_for_signal() -> std::io::Result<&'static str> {
    use tokio::signal::unix::{signal, SignalKind};

    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;

    tokio::select! {
        _ = interrupt.recv() => Ok("SIGINT"),
        _ = terminate.recv() => Ok("SIGTERM"),
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() -> std::io::Result<&'static str> {
    tokio::signal::ctrl_c().await?;
    Ok("CTRL+C")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn every_waiter_is_woken() {
        let shutdown = Shutdown::new();

        let waiters: Vec<_> = (0..3)
            .map(|_| {
                let s = shutdown.clone();
                tokio::spawn(async move { s.recv().await })
            })
            .collect();

        // Let the waiters reach `notified()` before signalling.
        tokio::time::sleep(Duration::from_millis(20)).await;
        shutdown.trigger();

        for waiter in waiters {
            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .expect("waiter should wake on shutdown")
                .expect("waiter task should not panic");
        }
    }

    #[tokio::test]
    async fn a_clone_shares_the_same_signal() {
        let shutdown = Shutdown::new();
        let clone = shutdown.clone();

        let handle = tokio::spawn(async move { clone.recv().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        shutdown.trigger();

        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("clone should observe the original's signal")
            .unwrap();
    }
}
