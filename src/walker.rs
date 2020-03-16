use async_std::fs;
use async_std::path::{PathBuf};
use async_std::stream::StreamExt;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::future::Future;
use std::pin::Pin;
use std::thread;

pub(crate) enum WalkerMessage {
    File(PathBuf),
    Quit,
}

pub(crate) struct Walker {
    sender: Sender<WalkerMessage>,
    dir_path: PathBuf,
}

pub(crate) type WalkerReceiver = Receiver<WalkerMessage>;

impl Walker {
    pub(crate) fn new(dir_path: PathBuf) -> (Self, WalkerReceiver) {
        let (sender, receiver) = unbounded();

        (Self { sender, dir_path }, receiver)
    }

    pub(crate) fn spawn(self) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let sender = self.sender.clone();
            let crawler = Crawler::new(self.dir_path, self.sender);
            async_std::task::block_on(async move {
                eprintln!("Crawling start.");
                crawler.crawl().await;
                sender.send(WalkerMessage::Quit).unwrap();
                eprintln!("Crawling end.");
            });
        })
    }
}

struct Crawler {
    root: PathBuf,
    sender: Sender<WalkerMessage>,
}

impl Crawler {
    fn new(root: PathBuf, sender: Sender<WalkerMessage>) -> Self {
        Self { root, sender }
    }

    fn crawl(self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        let sender = self.sender;
        let path = self.root;

        Box::pin(async move {
            let mut tasks = vec![];

            let mut dir_children = {
                if let Ok(children) = fs::read_dir(path).await {
                    children
                } else {
                    return;
                }
            };

            while let Some(dir_child) = dir_children.next().await {
                let sender = sender.clone();
                let dir_child = dir_child.expect("Failed to make dir child.").path();

                if dir_child.is_file().await {
                    // TODO: try wrapping this part in an async task
                    while sender
                        .try_send(WalkerMessage::File(dir_child.clone()))
                        .is_err()
                    {
                        async_std::task::yield_now().await;
                    }
                } else if dir_child.is_dir().await {
                    tasks.push(async_std::task::spawn(async move {
                        let crawler = Crawler::new(dir_child, sender);
                        crawler.crawl().await;
                    }));
                }
            }

            for task in tasks {
                task.await;
            }

            eprintln!("Crawling complete.");
        })
    }
}
