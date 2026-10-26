use super::*;
use crate::runtime::shared_runtime;
use op_ai::chat_provider::{ChatDelta, ChatProvider, ChatRequest};

#[test]
fn cancellable_bridge_unblocks_when_a_silent_provider_is_canceled() {
    let (tx, rx) = mpsc::channel(1);
    let task = shared_runtime().spawn(async move {
        let _keep_sender_alive = tx;
        std::future::pending::<()>().await;
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        signal.store(true, Ordering::Relaxed);
    });

    let started = std::time::Instant::now();
    let mut iter = BlockingRecvIter::<ChatDelta>::cancellable(rx, cancel, task);
    assert!(iter.next().is_none());
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "cancellation must not wait for another provider event"
    );
}

#[test]
fn cooperative_bridge_drops_receiver_without_aborting_provider_cleanup() {
    let (tx, rx) = mpsc::channel::<ChatDelta>(1);
    let (closed_tx, closed_rx) = std::sync::mpsc::channel();
    let cleanup_task = shared_runtime().spawn(async move {
        tx.closed().await;
        let _ = closed_tx.send(());
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&cancel);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        signal.store(true, Ordering::Release);
    });

    let mut iter = BlockingRecvIter::<ChatDelta>::cooperative(rx, cancel);
    assert!(iter.next().is_none());
    closed_rx
        .recv_timeout(Duration::from_millis(500))
        .expect("cooperative cancellation must wake tx.closed cleanup");
    block_on_anywhere(async {
        cleanup_task.await.expect("provider cleanup task exits");
    });
}

struct CleanupGuard(Arc<AtomicBool>);

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        // Make abort-without-join observably different from joined cleanup.
        std::thread::sleep(Duration::from_millis(20));
        self.0.store(true, Ordering::Release);
    }
}

fn guarded_worker(
    close_sender: bool,
) -> (
    mpsc::Receiver<ChatDelta>,
    tokio::task::JoinHandle<()>,
    Arc<AtomicBool>,
) {
    let (tx, rx) = mpsc::channel(1);
    let cleaned = Arc::new(AtomicBool::new(false));
    let guard = CleanupGuard(Arc::clone(&cleaned));
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let task = shared_runtime().spawn(async move {
        let _guard = guard;
        let _sender = if close_sender { None } else { Some(tx.clone()) };
        drop(tx);
        started_tx.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    started_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("worker owns its cleanup guard before cancellation");
    (rx, task, cleaned)
}

#[test]
fn joined_cancellation_waits_for_task_cleanup_before_returning() {
    let (rx, task, cleaned) = guarded_worker(false);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut iter = BlockingRecvIter::cancellable_joined(rx, Arc::clone(&cancel), task);
    cancel.store(true, Ordering::Release);
    assert!(iter.next().is_none());
    assert!(
        cleaned.load(Ordering::Acquire),
        "task guard still alive after next"
    );
    assert!(iter.next().is_none());
}

#[test]
fn joined_drop_waits_for_task_cleanup_before_returning() {
    let (rx, task, cleaned) = guarded_worker(false);
    let iter = BlockingRecvIter::cancellable_joined(rx, Arc::new(AtomicBool::new(false)), task);
    drop(iter);
    assert!(
        cleaned.load(Ordering::Acquire),
        "task guard still alive after drop"
    );
}

#[test]
fn joined_channel_end_waits_for_a_worker_that_dropped_its_sender() {
    let (rx, task, cleaned) = guarded_worker(true);
    let mut iter = BlockingRecvIter::cancellable_joined(rx, Arc::new(AtomicBool::new(false)), task);
    assert!(iter.next().is_none());
    assert!(
        cleaned.load(Ordering::Acquire),
        "closed channel left task running"
    );
}

#[test]
fn unsupported_joined_send_does_not_start_a_legacy_provider() {
    struct LegacyProvider;
    impl ChatProvider for LegacyProvider {
        fn provider_label(&self) -> &str {
            "legacy"
        }

        fn send(&self, _request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
            panic!("unsupported joined send must not delegate to send")
        }
    }
    assert!(!LegacyProvider.supports_joined_cancellable_send());
    assert!(LegacyProvider
        .send_cancellable_joined(ChatRequest::default(), Arc::new(AtomicBool::new(false)))
        .is_none());
}
