//! Blocking receiver bridge shared by native chat transports.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::runtime::block_on_anywhere;

/// Adapter that turns a `tokio::sync::mpsc::Receiver<ChatDelta>` into
/// a sync `Iterator<Item = ChatDelta>`. The receiver's
/// `blocking_recv` blocks the calling thread until a value arrives or
/// the channel closes — exactly the contract `ChatProvider::send`'s
/// iterator return needs. Sharing this helper across both BuiltIn +
/// Subprocess (and the future HttpServer / Acp) keeps the async ↔
/// sync bridge in one place.
pub struct BlockingRecvIter<T> {
    rx: Option<mpsc::Receiver<T>>,
    cancel: Option<Arc<AtomicBool>>,
    task: Option<tokio::task::JoinHandle<()>>,
    join_on_finish: bool,
}

impl<T> BlockingRecvIter<T> {
    pub fn new(rx: mpsc::Receiver<T>) -> Self {
        Self {
            rx: Some(rx),
            cancel: None,
            task: None,
            join_on_finish: false,
        }
    }

    /// Cancellation bridge for providers that own their worker lifecycle.
    /// When `cancel` is set, the receive half is dropped so a provider task
    /// watching `tx.closed()` can perform its own graceful transport cleanup.
    /// Unlike [`Self::cancellable`], this does not abort a Tokio task.
    pub fn cooperative(rx: mpsc::Receiver<T>, cancel: Arc<AtomicBool>) -> Self {
        Self {
            rx: Some(rx),
            cancel: Some(cancel),
            task: None,
            join_on_finish: false,
        }
    }

    /// A polling bridge for transports whose async task can be aborted. The
    /// short poll interval lets synchronous consumers observe cancellation
    /// even when the provider is silent and would otherwise block forever in
    /// `blocking_recv`.
    pub fn cancellable(
        rx: mpsc::Receiver<T>,
        cancel: Arc<AtomicBool>,
        task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            rx: Some(rx),
            cancel: Some(cancel),
            task: Some(task),
            join_on_finish: false,
        }
    }

    /// A cancellation bridge that waits for the aborted task's destructors.
    /// Consume or drop on a plain thread or a multi-thread runtime worker,
    /// matching the synchronous contract of [`block_on_anywhere`].
    pub fn cancellable_joined(
        rx: mpsc::Receiver<T>,
        cancel: Arc<AtomicBool>,
        task: tokio::task::JoinHandle<()>,
    ) -> Self {
        let mut iter = Self::cancellable(rx, cancel, task);
        iter.join_on_finish = true;
        iter
    }

    fn abort_worker(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            if self.join_on_finish {
                block_on_anywhere(async {
                    let _ = task.await;
                });
            }
        }
    }
}

impl<T> Iterator for BlockingRecvIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        if self.cancel.is_none() {
            return self.rx.as_mut()?.blocking_recv();
        }
        // Timed blocking bridge: tokio's mpsc has no blocking recv with a
        // timeout, so poll the receiver with a waker that unparks this
        // thread. A message (or channel close) ends the park immediately;
        // the 20 ms cap keeps the cancel flag observed even when the
        // provider is silent.
        let waker = Waker::from(Arc::new(ThreadUnparker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        loop {
            if self
                .cancel
                .as_ref()
                .is_some_and(|cancel| cancel.load(Ordering::Relaxed))
            {
                // Release the receiver before returning so cooperative
                // provider tasks waiting on `Sender::closed` wake promptly.
                self.rx.take();
                self.abort_worker();
                return None;
            }
            match self.rx.as_mut()?.poll_recv(&mut cx) {
                Poll::Ready(Some(value)) => return Some(value),
                Poll::Ready(None) => {
                    if self.join_on_finish {
                        self.abort_worker();
                    }
                    return None;
                }
                Poll::Pending => std::thread::park_timeout(Duration::from_millis(20)),
            }
        }
    }
}

/// Wakes [`BlockingRecvIter::next`]'s park as soon as a message lands.
struct ThreadUnparker(std::thread::Thread);

impl Wake for ThreadUnparker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

impl<T> Drop for BlockingRecvIter<T> {
    fn drop(&mut self) {
        self.rx.take();
        self.abort_worker();
    }
}

#[cfg(test)]
#[path = "blocking_recv_tests.rs"]
mod tests;
