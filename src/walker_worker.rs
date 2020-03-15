use crate::walker::{WalkerMessage, WalkerReceiver};
use async_std::path::PathBuf;
use async_std::stream::StreamExt;
use async_trait::async_trait;
use crossbeam_channel::{unbounded, TryRecvError};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// A trait describing a work handler,
/// which knows how to work given a pathbuf that needs working.
#[async_trait]
pub(crate) trait WorkHandler: Clone {
    async fn handle_work(&mut self, path: PathBuf);
}

/// A worker that can read from the VisitQueue,
/// perform some work on the visited path,
/// and possibly push yet another path into the VisitQueue.
pub(crate) struct WalkerWorker<T: WorkHandler> {
    receiver: WalkerReceiver,
    work_handler: T,
    work_pending: Arc<AtomicUsize>,
}

impl<T: WorkHandler> WalkerWorker<T> {
    fn new(work_handler: T, receiver: WalkerReceiver, work_pending: Arc<AtomicUsize>) -> Self {
        Self {
            receiver,
            work_handler,
            work_pending,
        }
    }

    async fn start_working(&mut self) {
        loop {
            let message = match self.receiver.try_recv() {
                Err(TryRecvError::Disconnected) => {
                    return;
                }
                Err(TryRecvError::Empty) => {
                    async_std::task::yield_now().await;
                    continue;
                }
                Ok(message) => message,
            };

            // eprintln!("Sendqueue count: {}", self.visit_queue.sender.len());
            // eprintln!("Receivequeue count: {}", self.visit_queue.receiver.len());

            match message {
                WalkerMessage::File(path) => {
                    self.work_handler.handle_work(path).await;
                }
                WalkerMessage::Quit => {}
            }
        }
    }
}

pub(crate) struct WorkerPool<T: WorkHandler> {
    _workers: Vec<WalkerWorker<T>>,
}

impl<T: WorkHandler + Send + Sync + 'static> WorkerPool<T> {
    pub(crate) async fn spawn(
        handler: T,
        path: PathBuf,
        rcv: WalkerReceiver,
        workers_count: usize,
    ) {
        assert!(workers_count > 0);

        let mut work_vec = vec![];

        let work_pending = Arc::new(AtomicUsize::new(1));

        for _ in 0..workers_count {
            let mut worker = WalkerWorker::new(handler.clone(), rcv.clone(), work_pending.clone());

            work_vec.push(async_std::task::spawn(async move {
                worker.start_working().await
            }));
        }

        drop(rcv);

        for work in work_vec {
            work.await;
        }
    }
}
