use async_std::path::PathBuf;
use async_std::stream::StreamExt;
use crossbeam_channel::unbounded;

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
pub(crate) struct WalkerWorker<W>
where
    W: FnMut(PathBuf),
{
    visit_queue: VisitQueue<PathBuf>,
    work_handler: W,
}

impl<W> WalkerWorker<W>
where
    W: FnMut(PathBuf),
{
    fn new(visit_queue: VisitQueue<PathBuf>, work_handler: W) -> Self {
        Self {
            visit_queue,
            work_handler,
        }
    }

    async fn start_working(&mut self) {
        while let Some(path) = self.visit_queue.pop_visit_blocking() {
            if path.is_file().await {
                (self.work_handler)(path);
            } else if path.is_dir().await {
                let mut dir_stream = path.read_dir().await.unwrap();

                while let Some(child) = dir_stream.next().await {
                    self.visit_queue.push_visit_blocking(child.unwrap().path());
                }
            }
        }
    }
}

pub(crate) struct WorkerPool<W>
where
    W: FnMut(PathBuf),
{
    workers: Vec<WalkerWorker<W>>,
}

impl<W> WorkerPool<W>
where
    W: FnMut(PathBuf) + std::marker::Send + Clone + 'static,
{
    pub(crate) async fn spawn(path: PathBuf, initial_count: usize, work_handler: W) -> Self {
        let mut workers = vec![];

        let sharable_queue = VisitQueue::new();

        sharable_queue.push_visit_blocking(path);

        let mut spawned_tasks = vec![];

        for _ in 0..initial_count {
            let mut worker = WalkerWorker::new(sharable_queue.clone(), work_handler.clone());

            println!("Starting work.");

            spawned_tasks.push(async_std::task::spawn(async move {
                worker.start_working().await
            }));
        }

        println!("Waiting for work to finish.");
        for task in spawned_tasks {
            task.await;
        }

        println!("Done working.");

        Self { workers }
    }
}
