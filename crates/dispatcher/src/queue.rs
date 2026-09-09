//! The bounded queue between fetching and processing.
//!
//! A thin wrapper over a Tokio channel, kept as its own type for two reasons: the
//! depth is a metric the admin API reports, and `send` blocking when full *is* the
//! backpressure signal, so it should be obvious at the call site rather than
//! hidden in a channel handle.

use tokio::sync::mpsc;

/// A block waiting to be processed.
#[derive(Debug)]
pub struct QueuedBlock<B> {
    /// Its height.
    pub height: u64,
    /// The fetched block.
    pub block: B,
}

/// Sending half of the block queue.
#[derive(Debug, Clone)]
pub struct QueueSender<B> {
    inner: mpsc::Sender<QueuedBlock<B>>,
}

/// Receiving half of the block queue.
#[derive(Debug)]
pub struct QueueReceiver<B> {
    inner: mpsc::Receiver<QueuedBlock<B>>,
}

/// Create a bounded queue.
pub fn queue<B>(capacity: usize) -> (QueueSender<B>, QueueReceiver<B>) {
    let (tx, rx) = mpsc::channel(capacity.max(1));
    (QueueSender { inner: tx }, QueueReceiver { inner: rx })
}

impl<B> QueueSender<B> {
    /// Enqueue a block, waiting while the queue is full.
    ///
    /// The wait is the backpressure: a fetcher blocked here is a fetcher not
    /// pulling more blocks into memory.
    ///
    /// Returns `false` once the receiver is gone (shutdown).
    pub async fn send(&self, height: u64, block: B) -> bool {
        self.inner.send(QueuedBlock { height, block }).await.is_ok()
    }

    /// Blocks currently queued.
    pub fn depth(&self) -> usize {
        self.inner.max_capacity() - self.inner.capacity()
    }

    /// Free slots.
    pub fn free(&self) -> usize {
        self.inner.capacity()
    }

    /// Whether the queue is at capacity.
    pub fn is_full(&self) -> bool {
        self.inner.capacity() == 0
    }
}

impl<B> QueueReceiver<B> {
    /// Take the next block, or `None` once every sender has been dropped.
    pub async fn recv(&mut self) -> Option<QueuedBlock<B>> {
        self.inner.recv().await
    }

    /// Discard everything queued, returning how many were dropped.
    ///
    /// Used on a reorg, where queued blocks belong to the abandoned branch.
    pub fn drain(&mut self) -> usize {
        let mut dropped = 0;
        while self.inner.try_recv().is_ok() {
            dropped += 1;
        }
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn blocks_come_out_in_the_order_they_went_in() {
        let (tx, mut rx) = queue::<u64>(8);
        for h in 100..103 {
            assert!(tx.send(h, h).await);
        }
        for expected in 100..103 {
            assert_eq!(rx.recv().await.unwrap().height, expected);
        }
    }

    #[tokio::test]
    async fn depth_and_free_track_occupancy() {
        let (tx, mut rx) = queue::<u64>(4);
        assert_eq!(tx.depth(), 0);
        assert_eq!(tx.free(), 4);

        tx.send(1, 1).await;
        tx.send(2, 2).await;
        assert_eq!(tx.depth(), 2);
        assert_eq!(tx.free(), 2);
        assert!(!tx.is_full());

        tx.send(3, 3).await;
        tx.send(4, 4).await;
        assert!(tx.is_full());

        rx.recv().await;
        assert!(!tx.is_full());
    }

    #[tokio::test]
    async fn draining_discards_the_abandoned_branch() {
        let (tx, mut rx) = queue::<u64>(8);
        for h in 100..105 {
            tx.send(h, h).await;
        }
        assert_eq!(rx.drain(), 5);
        assert_eq!(tx.depth(), 0);
    }

    #[tokio::test]
    async fn sending_after_shutdown_reports_failure() {
        let (tx, rx) = queue::<u64>(4);
        drop(rx);
        assert!(!tx.send(1, 1).await);
    }
}
