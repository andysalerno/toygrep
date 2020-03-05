use async_std::path::PathBuf;
use async_std::stream::StreamExt;
use async_trait::async_trait;
use crossbeam_channel::unbounded;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// A trait describing a work handler,
/// which knows how to work given a pathbuf that needs working.
#[async_trait]
pub(crate) trait WorkHandler: Clone {
    async fn handle_work(&mut self, path: PathBuf);
}

/// A queue of paths for workers to visit.
/// Workers can read from, and push to, this queue.
#[derive(Clone)]
struct MessageQueue<T> {
    sender: crossbeam_channel::Sender<WorkMessage<T>>,
    receiver: crossbeam_channel::Receiver<WorkMessage<T>>,
}

impl<T> MessageQueue<T> {
    fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }

    /// Push a new visit onto the queue, possibly
    /// blocking the thread until the push succeeds.
    fn push_message_blocking(&self, message: WorkMessage<T>) {
        self.sender.send(message).ok();
    }

    /// Block until
    /// it succeeds, and returning None if the connection closes.
    fn pop_message_blocking(&self) -> Option<WorkMessage<T>> {
        self.receiver.recv().ok()
    }
}

enum WorkMessage<T> {
    Visit(T),
    Quit,
}

/// A worker that can read from the VisitQueue,
/// perform some work on the visited path,
/// and possibly push yet another path into the VisitQueue.
pub(crate) struct WalkerWorker<T: WorkHandler> {
    visit_queue: MessageQueue<PathBuf>,
    work_handler: T,
    work_pending: Arc<AtomicUsize>,
}

impl<T: WorkHandler> WalkerWorker<T> {
    fn new(
        work_handler: T,
        visit_queue: MessageQueue<PathBuf>,
        work_pending: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            visit_queue,
            work_handler,
            work_pending,
        }
    }

    async fn start_working(&mut self) {
        while let Some(message) = self.visit_queue.pop_message_blocking() {
            match message {
                WorkMessage::Visit(path) => {
                    if path.is_file().await {
                        self.work_handler.handle_work(path).await;
                    } else if path.is_dir().await {
                        let mut dir_stream = path.read_dir().await.unwrap();

                        let mut new_work = vec![];

                        while let Some(child) = dir_stream.next().await {
                            new_work.push(WorkMessage::Visit(child.unwrap().path()));
                        }

                        self.work_pending.fetch_add(new_work.len(), Ordering::SeqCst);

                        new_work.into_iter().for_each(|w| {
                            self.visit_queue.push_message_blocking(w);
                        });
                    }

                    let prev_val = self.work_pending.fetch_sub(1, Ordering::SeqCst);

                    if prev_val == 1 {
                        // was 1, we subtracted to 0, so now it is 0
                        self.visit_queue.push_message_blocking(WorkMessage::Quit);
                        return;
                    }
                }
                WorkMessage::Quit => {
                    self.visit_queue.push_message_blocking(WorkMessage::Quit);
                    return;
                }
            }
        }
    }
}

pub(crate) struct WorkerPool<T: WorkHandler> {
    workers: Vec<WalkerWorker<T>>,
}

impl<T: WorkHandler + Send + Sync + 'static> WorkerPool<T> {
    pub(crate) async fn spawn(handler: T, path: PathBuf, initial_count: usize) -> Self {
        assert!(initial_count > 0);

        let mut workers = vec![];

        let mut sharable_queue = MessageQueue::new();

        sharable_queue.push_message_blocking(WorkMessage::Visit(path));

        let mut work_vec = vec![];

        let work_pending = Arc::new(AtomicUsize::new(1));

        for _ in 0..initial_count {
            let mut worker = WalkerWorker::new(
                handler.clone(),
                sharable_queue.clone(),
                work_pending.clone(),
            );

            work_vec.push(async_std::task::spawn(async move {
                worker.start_working().await
            }));
        }

        for work in work_vec {
            work.await;
        }

        Self { workers }
    }
}
