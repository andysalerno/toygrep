use async_std::path::PathBuf;
use async_std::stream::StreamExt;
use crossbeam_channel::unbounded;

/// A trait describing a work handler,
/// which knows how to work given a pathbuf that needs working.
pub(crate) trait WorkHandler: Clone {
    fn handle_work(&self, work: PathBuf);
}

/// A queue of paths for workers to visit.
/// Workers can read from, and push to, this queue.
#[derive(Clone)]
struct VisitQueue<T> {
    sender: crossbeam_channel::Sender<T>,
    receiver: crossbeam_channel::Receiver<T>,
}

impl<T> VisitQueue<T> {
    fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self { sender, receiver }
    }

    /// Push a new visit onto the queue, possibly
    /// blocking the thread until the push succeeds.
    fn push_visit_blocking(&self, visit: T) {
        self.sender.send(visit).ok();
    }

    /// Pop a visit from the queue, blocking until
    /// it succeeds, and returning None if the connection closes.
    fn pop_visit_blocking(&self) -> Option<T> {
        self.receiver.recv().ok()
    }
}

/// A worker that can read from the VisitQueue,
/// perform some work on the visited path,
/// and possibly push yet another path into the VisitQueue.
pub(crate) struct WalkerWorker<T: WorkHandler> {
    visit_queue: VisitQueue<PathBuf>,
    work_handler: T,
}

impl<T: WorkHandler> WalkerWorker<T> {
    fn new(work_handler: T, visit_queue: VisitQueue<PathBuf>) -> Self {
        Self {
            visit_queue,
            work_handler,
        }
    }

    async fn start_working(self) {
        while let Some(path) = self.visit_queue.pop_visit_blocking() {
            if path.is_file().await {
                self.work_handler.handle_work(path);
            } else if path.is_dir().await {
                let mut dir_stream = path.read_dir().await.unwrap();

                while let Some(child) = dir_stream.next().await {
                    self.visit_queue.push_visit_blocking(child.unwrap().path());
                }
            }
        }
    }
}

pub(crate) struct WorkerPool<T: WorkHandler> {
    workers: Vec<WalkerWorker<T>>,
}

impl<T: WorkHandler> WorkerPool<T> {
    pub(crate) fn spawn(handler: T, initial_count: usize) -> Self {
        let mut workers = vec![];

        let sharable_queue = VisitQueue::new();

        for _ in 0..initial_count {
            let worker = WalkerWorker::new(handler.clone(), sharable_queue.clone());
            workers.push(worker);
        }

        Self { workers }
    }
}
